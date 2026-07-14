# M5 — Remaining point pattern gaps

## Status

Draft — requires a finding-specific `grill-with-docs` session before each small
vertical TDD slice.

## Finding

The H3 family is closed, but four separately scoped forms remain uncovered:
recursive `chmod 000`, `TRUNCATE` without `TABLE`, `docker volume rm`, and
`npm publish`.

## Rule decisions to validate during implementation

- Filesystem: program-led `chmod` form should use token-prefix semantics where
  its argument ordering can be modeled without a broad regex.
- SQL: `TRUNCATE` is embedded SQL and follows ADR-015 match-anywhere behavior;
  use narrow boundaries to avoid identifiers/log text.
- Docker and npm: program-led verbs follow ADR-014 prefix normalization and must
  survive launcher/absolute-path forms.
- Risk levels and descriptions must be justified against sibling rules before
  code is written.

## TDD seams

For each rule, add one must-fire public `Scanner::assess` example and at least
one near-miss. Then add the self-validating built-in `match_examples` and
`not_match_examples`. Work one rule at a time; do not bulk-add all tests first.

## Implementation sequence

1. Grill and land the `chmod` rule.
2. Grill and land the SQL rule within ADR-015 limits.
3. Land Docker, then npm prefix rules.
4. Run the eval harness and inspect false-positive changes.

## Verification

- Built-in example validation and focused scanner regressions
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- scanner benchmark if quick-pass keywords or indexes change
