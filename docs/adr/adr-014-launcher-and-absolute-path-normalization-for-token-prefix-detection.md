# ADR-014: Launcher and absolute-path normalization for token-prefix detection

## Status

Accepted

## Context

Git, Database, Cloud, Docker, and some Process detections were migrated from
regex `Pattern`s to `Token-prefix rule`s. `Scanner::prefix_scan` looks a rule up
by the literal `tokens[0]` of each scan target, with no normalization. Two
classes of input therefore bypass every migrated family:

- **Absolute paths** — `/usr/bin/git reset --hard` has first token
  `/usr/bin/git`, which matches no rule keyed on `git`.
- **Launcher/wrapper prefixes** — `sudo`, `env`, `nice`, `timeout`, the
  site-specific `rtk`, etc. shift the real program off position 0. Only `env`
  and `VAR=value` assignments were ever stripped (`strip_env_prefix`); `sudo`,
  `rtk`, and the rest were not.

This is asymmetric with the surviving regex families, which match anywhere in
the normalized string and so are unaffected by a prefix. The asymmetry is a
regression introduced by the migration, and it is not limited to a prompt
bypass: `kill -9 1` (SIGKILL to PID 1) is a `Block`-level token-prefix rule, so
`sudo kill -9 1` — the only realistic form, since killing init needs `sudo` —
defeats the intrinsic, unbreakable `Block` guarantee.

The `quick_scan` hot-path gate is not the leak: first tokens of prefix rules are
seeded into the Aho-Corasick automaton and matched as substrings, so a prefixed
command still reaches the full scan. The miss is strictly in the prefix-rule
lookup key (and the by-program regex index, where universal regexes still cover).

## Decision

Resolve each scan target to its **`Effective program`** — strip launcher
prefixes and take the basename of an absolute path — and use that as the lookup
key for token-prefix rules and the by-program regex index.

1. **One normalizer in `aegis-parser`**, applied **per scan target** (raw
   command, each logical segment, each inline script), not once on the
   top-level command. `ParsedCommand.program`/`normalized`/`raw` are not
   mutated — the audit log keeps the raw spelling.
2. **Launcher stripping uses a built-in option-arity table** (`sudo`, `env`,
   `command`, `nice`, `timeout`, `nohup`, `time`, `doas`, …), applied
   recursively for chains (`sudo env … git`). On an **unknown** launcher flag,
   resolution is conservative — both candidate readings are considered, so a
   prefixed dangerous program is matched rather than silently dropped. Known
   value-taking timeout flags (`-s`/`--signal`, `-k`/`--kill-after`) are parsed
   before the mandatory duration; sudo environment assignments (`sudo FOO=bar
   git ...`) are treated as launcher metadata rather than as the effective
   program; stacked sudo options continue scanning after conservative forks.
3. **Site-specific launchers** that are already part of Aegis' supported agent
   workflow (`rtk`) are included in the built-in launcher table. Configurable
   launcher extensions remain a future contract; if introduced, project-layer
   entries must be untrusted and collision-checked against known detection
   program tokens (`git`, `kill`, `docker`, …).

Normalization runs only inside the post-`quick_scan` target loop, so the
`Safe`-path hot budget (< 2 ms, ADR-002) is unaffected.

## Consequences

Token-prefix families can no longer be bypassed by an absolute path, a `sudo`/
`env`/`rtk`/`nice`/`timeout` prefix, or a chain of them; in particular the
intrinsic `Block` on `kill -9 1` survives `sudo`. Conservative handling of
unknown launcher flags can produce an extra prompt rather than a miss — an
intentional fail-closed trade-off. Launcher normalization exists to *recognize*
wrappers, never to *exempt* a program from detection.

This ADR is scoped to first-token normalization. Compound-command segmentation
gaps are tracked separately (TASKS.md H1); `logical_segments` already splits on
`&&`/`;`/`|` and `prefix_scan` runs per segment, so `cd dir && git reset --hard`
is expected to already match and is ground-truthed by a regression test rather
than folded into this change.
