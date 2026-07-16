# Aegis

A `$SHELL` proxy that intercepts AI-agent commands and requires human confirmation
before destructive operations. This file is the **domain glossary** — the ubiquitous
language for the project. It is the single source of truth that humans, AI agents, and
the code itself (type names, fields, config keys, audit fields) must all use for the
same concept. It is maintained by the `domain-modeling` skill and contains no
implementation details. Keep definitions tight and opinionated.

## Command

**Parsed command**:
The canonical token-level representation the scanner consumes (`ParsedCommand`). Carries
`program`, `argv`, the `normalized` form, extracted `inline_scripts`, and the original
`raw` string. Every scanner stage works on this, never on the raw string.
_Avoid_: tokenized command, command struct

**Normalized command**:
The de-quoted, space-joined token sequence (`ParsedCommand.normalized`) — the scanner's
primary match target, free of shell quoting and escape noise.
_Avoid_: cleaned command, sanitized command

**Inline script**:
A script body extracted from an interpreter invocation (`python -c`, `node -e`),
scanned in its own right so risky code hidden behind an interpreter flag is still caught.
_Avoid_: embedded script, subcommand

**Indirect execution**:
Running commands through an interpreter or another layer (inline scripts, piping into a
shell) rather than invoking the program directly. `Strict` mode blocks it.
_Avoid_: nested execution, eval

**Script-file execution**:
Running an interpreter against a script _file_ named in argv (`sh ./cleanup.sh`,
`python3 ./x.py`, `node ./x.js`, `source ./x`) — the destructive effect lives in the file,
which Aegis does not read at classification time. A sub-case of `Indirect execution` and
the sibling of `Inline script`: an inline body (`-c` / `-e`) is extracted and scanned, but
a referenced file is not.
_Avoid_: external script, script bypass, file exec

**Effect-opaque execution**:
A command shape whose text reveals that another execution layer will decide the eventual
filesystem, database, or network effect, but does not reveal that effect directly.
`Script-file execution` is effect-opaque; an `Inline script` may stop being
effect-opaque once its body is extracted and assessed. Orthogonal to `RiskLevel`.
_Avoid_: hidden effect, opaque command, unknown execution

**Launcher prefix**:
A leading token that launches another program rather than being the target itself
(`sudo`, `env`, `nice`, `timeout`, `command`, the site-specific `rtk`, …). Stripped — with
its options, via a built-in option-arity table — to expose the real program for **detection
matching only** (never for execution). Built-in launchers are trusted and include
the local `rtk` execution wrapper. Distinct from the `Wrapper` (`$SHELL` proxy) and
from a `Hook`.
_Avoid_: wrapper, command wrapper, exec prefix

**Effective program**:
The real program token a scan target resolves to after stripping launcher prefixes and
taking the basename of an absolute path (`/usr/bin/git` → `git`, `sudo rtk git` → `git`).
Computed per scan target and used as the lookup key for `Token-prefix rule`s and the
by-program regex index — so prefixes and absolute paths cannot bypass a rule keyed on the
first token. Distinct from `ParsedCommand.program`, which preserves the raw leading token.
_Avoid_: real program, resolved command, normalized program

**Logical segment**:
A scan-oriented command unit produced by `logical_segments` — the raw string cut at
top-level `Command separator`s and normalized, so each independent command is assessed
on its own. A scan-time boundary, not an execution unit: fork semantics of a background
`&` are ignored; it only marks where one command ends.
_Avoid_: segment, sub-command, command part

**Command separator**:
A top-level shell control operator that ends one `Logical segment` and starts the next:
`;`, `&&`, `||`, `|`, newline, and a standalone background `&`. A `&` that is part of a
redirect (`&>`, `>&`, `2>&1`) is not a separator.
_Avoid_: delimiter, control operator

**Short flag bundle**:
A single shell token that combines multiple one-letter CLI flags (for example `-af` as
`-a` + `-f`). Aegis treats bundle semantics as command-specific unless a rule explicitly
models them; exact flag tokens remain the default for `Token-prefix rule`s.
_Avoid_: combined flags, packed flags

## Scanner

**Assessment**:
The result of scanning one command — a `RiskLevel` plus the patterns it matched and
the parsed command.
_Avoid_: result, verdict, scan output

**RiskLevel**:
The severity a command is classified as, ordered by escalation: `Safe`, `Warn`,
`Danger`, `Block`. The order is semantic — never reorder it.
_Avoid_: severity level, threat level

**Intrinsic Block**:
A hard-coded, unbypassable `Block` decision checked before allowlist, rules, and mode.
The product's core guarantee that certain commands never execute.
_Avoid_: hard block, force block

**Pattern**:
A regex-based detection rule (built-in or user-defined) matched against the
normalized command string; matches **anywhere** in the string. Database rules are
regex `Pattern`s (match-anywhere), not `Token-prefix rule`s: SQL verbs (`DROP TABLE`)
arrive embedded in `psql -c` / `mysql -e` / heredoc / stdin, not as a leading program
token (ADR-015).
_Avoid_: rule, signature (reserve "rule" for prefix rules)

**Token-prefix rule**:
A detection rule keyed on a command's `Effective program` token (e.g. `git`, `docker`) and
matched against the token sequence — distinct from a regex `Pattern`. Git, Cloud,
Docker, some Process, and some Filesystem rules (`wipefs`, `unlink` — where the
destructive verb *is* the effective program) are token-prefix rules. A destructive
operation that arrives embedded mid-command instead (SQL verbs, a redirect to a
sensitive path) stays a regex `Pattern` (ADR-014/015).
_Avoid_: prefix pattern, first-token rule

**Quick scan**:
The fast first pass — an Aho-Corasick multi-pattern scan with no allocations, on the
< 2ms hot path. Never uses regex.
_Avoid_: prefilter, fast match

**Full scan**:
The verification pass that runs regex `Pattern`s and token-prefix rules after the
quick scan flags a candidate.
_Avoid_: deep scan, second pass

**Category**:
The domain a detection rule belongs to: `Filesystem`, `Git`, `Database`, `Cloud`,
`Docker`, `Process`, `Package`.

**Match**:
One pattern hit (`MatchResult`) — the `Pattern` that fired, the text fragment that
triggered it, and the highlight span in the original command.
_Avoid_: hit, finding

**Decision source**:
What produced an assessment (`DecisionSource`): `BuiltinPattern`, `CustomPattern`, or
`Fallback` (nothing matched → assessed `Safe`). Distinct from the final `Decision`.
_Avoid_: origin, cause

## Policy

**Mode**:
The top-level posture: `Protect` (default — prompt on `Warn`/`Danger`), `Audit`
(non-blocking, log only), `Strict` (block non-safe and indirect execution).
_Avoid_: level, profile

**Allowlist / Blocklist**:
User-configured exceptions. Blocklist always wins over allowlist; an allowlist entry
only downgrades up to `allowlist_override_level`.
_Avoid_: whitelist, blacklist

**Override level**:
The ceiling an allowlist entry may downgrade to (`AllowlistOverrideLevel`): `Warn`
(default), `Danger`, or `Never`. Above the ceiling, the allowlist does not auto-approve.
_Avoid_: allow ceiling, max downgrade

**Policy rule**:
A typed `[[rules]]` entry in config whose outcome is a `PolicyRuleDecision` — `Allow`,
`Prompt`, or `Block`. A rule `Allow` auto-approves the command ahead of `Mode` and with
no `allowlist_override_level` ceiling — unlike an `[[allow]]` allowlist entry, which is
capped by the override level. Because that makes a project-layer `Allow` a silent
auto-approve of a `Warn`/`Danger` command, project-layer `[[rules]] Allow` is untrusted:
the ratchet drops it and `config validate` warns (ADR-013); a project that needs an
auto-approve must declare it in global config. This `Block` is a *rule outcome*, distinct
from the `Block` `RiskLevel` and from a blocklist entry.
_Avoid_: custom rule (reserve "rule" wording for prefix rules / Patterns)

**CI policy**:
What Aegis does in a detected CI environment (default `Block`, since no TTY exists to
prompt). Distinct from `Mode`.
_Avoid_: CI mode

**Snapshot policy**:
When snapshot plugins run (`SnapshotPolicy`): `None`, `Selective` (honour per-plugin
flags — default), or `Full` (run every registered plugin). Distinct from `Mode` and
`CI policy`.
_Avoid_: backup policy

## Decision & Execution

**Decision**:
The recorded outcome of the interception flow: `Approved`, `Denied`, `AutoApproved`,
`Blocked`, `Pruned`. The final human-or-auto verdict the audit log stores — distinct
from the scanner's `Assessment`.
_Avoid_: result, outcome, verdict (those belong to the scanner stage)

**Toggle**:
The global on/off switch checked at command boundaries; when off, Aegis passes commands
through unguarded (ADR-005).
_Avoid_: enable flag, kill switch

**Sandbox**:
An OS-level confinement profile optionally applied to an approved command before it
executes. A best-effort write/network guardrail add-on, not a security or
confidentiality boundary; it does not promise that file reads or secrets are
hidden from the command.
_Avoid_: jail, container

**Sandbox status**:
The confinement path selected during command preparation (`SandboxStatus`),
recorded in every audit entry: `Active`, `Unavailable`, `NotConfigured`, or
`NotAttempted`. `Active` means the confined launch path was prepared, not that a
later OS-level exec or spawn succeeded. `NotConfigured` means Sandbox was
disabled; `NotAttempted` means it was enabled but neither a confined nor
fallback launch path was used, including fail-closed preparation errors.
_Avoid_: sandbox state

**Sandbox bypass**:
Optional execution through the prepared unconfined fallback after Sandbox
infrastructure was unavailable (`SandboxStatus::Unavailable`). Required
unavailability blocks instead and is not a Sandbox bypass.
_Avoid_: sandbox failure, escape

## Snapshot & Audit

**Snapshot**:
A best-effort pre-execution capture (e.g. `git stash`) produced by an applicable
`Snapshot plugin`. It preserves only the state that plugin captures at that
moment; it is not a complete backup and does not promise to reverse every later
command effect.
_Avoid_: backup, checkpoint

**Snapshot plugin**:
A per-backend snapshotter (`git`, `docker`, `postgres`, `mysql`, `supabase`, `sqlite`)
that knows how to capture and restore state for its domain. Each successful run yields a
`SnapshotRecord` (`plugin` + opaque `snapshot_id`).
_Avoid_: snapshotter, driver, backend

**Snapshot store**:
The trusted directory a `Snapshot plugin` owns for reading and writing its
artifacts. A filesystem artifact must resolve beneath this directory before a
rollback or deletion may use it.
_Avoid_: snapshots dir, bundle root, snapshot root

**Snapshot artifact**:
The concrete filesystem object in a `Snapshot store` addressed by a
`snapshot_id`.
_Avoid_: dump, blob

**Path containment**:
The invariant that a resolved `Snapshot artifact` is provably beneath its
`Snapshot store`, including after symlink resolution.
_Avoid_: path validation, path sanitization

**Owner-only artifact permissions**:
The Unix invariant that a `Snapshot store` and its directory artifacts use mode
`0700`, while file `Snapshot artifact`s use mode `0600`; an unsafe store leaf
is tightened only when the current owner owns it, otherwise rejected before a
sensitive write.
_Avoid_: private snapshot, chmod security

**Required recovery**:
The obligation to create at least one `Snapshot` before executing a command.
The obligation is independent of whether any `Snapshot plugin` is available or
succeeds; an explicit trusted opt-out means recovery is not required rather than
degraded.
_Avoid_: mandatory backup, available snapshot

**Recovery degradation**:
The state where `Required recovery` applies but no `Snapshot` was created. It is
distinct from an explicit trusted recovery opt-out and must never silently become
permission to execute.
_Avoid_: snapshot warning, best-effort failure

**Recovery status**:
The post-attempt state of `Required recovery`: `Ready` when at least one
`Snapshot` was created, or `Degraded` when none was created. Execution surfaces
derive their deny or `Recovery override` behavior from this shared fact.
_Avoid_: snapshot result, recovery verdict

**Recovery override**:
A one-time human approval to execute despite a visible `Recovery degradation`.
It cannot be persisted as an allowlist entry because it applies to the observed
failure to create a `Snapshot`, not to the command prefix.
_Avoid_: always allow, recovery bypass

**Rollback**:
Restoring the state captured by a previous `Snapshot`, addressed by its
`snapshot_id`. It restores captured state; it is not a general undo of the
command that ran afterward.
_Avoid_: undo, revert

**Audit log**:
The append-only JSONL record at `~/.aegis/audit.jsonl`. One `AuditEntry` per line;
never rewritten. The format is part of the public contract.
_Avoid_: history, journal

**Audit directory**:
A directory Aegis creates while materializing the configured `Audit log` path.
A pre-existing parent remains a caller-owned container, not an Audit directory.
_Avoid_: audit parent, log folder

**Audit artifact**:
An owner-only filesystem object used by the audit subsystem: the active `Audit
log`, its lock file, a rotated segment, or the managed gzip rotation staging
object.
_Avoid_: audit file, log artifact

**Audit integrity chain**:
The optional unkeyed SHA-256 link between consecutive `AuditEntry` values and
rotated segments (`ChainSha256`). It detects corruption and inconsistent edits,
but has no keyed or external anchor and therefore does not prove adversarial
tamper-evidence against an actor who can rewrite the whole local log.
_Avoid_: tamper-evident log, tamper-proof audit

**AuditEntry**:
One JSONL line in the audit log — the structured record of a single intercepted command,
its `Decision`, and its `Sandbox status`.
_Avoid_: log line, event

## Surfaces

**Wrapper / `$SHELL` proxy**:
The aegis binary acting as the user's `$SHELL`, intercepting commands launched via
`$SHELL -c`. The shell-level surface, distinct from a per-agent `Hook`.
_Avoid_: shim (reserve "shim"/"hook" for per-agent routing)

**Hook**:
A per-agent shim (`claude-code.sh`, the Codex hook) that routes a tool call through
Aegis. Must fail **closed** (deny) on missing dependencies or invalid input.
_Avoid_: wrapper, plugin (reserve "wrapper" for the shell `$SHELL` proxy itself)
