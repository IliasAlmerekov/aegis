# Security Policy

## Supported versions

Aegis is pre-1.0.

Security fixes are targeted at:

- the latest commit on `main`
- the latest published release, when one exists

Older commits, abandoned branches, and local forks are not supported for coordinated disclosure.

## Reporting a vulnerability

Please **do not** report suspected vulnerabilities in public GitHub issues, pull requests, or Discussions.

Preferred channel:

1. Use GitHub's private vulnerability reporting flow from the repository's **Security** tab, if it is available for this repository.

Fallback if private reporting is unavailable:

1. Contact the maintainer through the contact links on the GitHub profile: <https://github.com/IliasAlmerekov>
2. Include:
   - a short summary
   - affected version / commit
   - reproduction steps or proof of concept
   - impact assessment
   - any suggested mitigation

Please keep details private until a fix or mitigation is available.

## What is in scope

The following are in scope for responsible disclosure:

- fail-open behavior that lets dangerous commands execute without the intended approval gate
- bypasses of `Block`-level protections
- allowlist or CI-mode bypasses that weaken documented safety guarantees
- approval-flow bugs that silently auto-approve or skip confirmation
- audit-log integrity issues, including missing, forged, or silently dropped security-relevant entries
- snapshot / rollback contract bugs that create false safety signals for dangerous commands
- installer or release-flow issues that could compromise shipped binaries or update paths

## What is out of scope

The following are currently out of scope because they are documented non-goals or not product vulnerabilities by themselves:

- feature requests, usability feedback, and general support questions
- documentation typos or wording-only issues with no security impact
- reports that depend on the user explicitly approving a dangerous command after being warned
- issues limited to local development helper directories such as `.claude/`, `.codex/`, or `.planning/`
- vulnerabilities that exist only in third-party infrastructure outside this repository unless Aegis introduces the exploitable condition

The product also has explicit design limitations documented in `README.md` and `docs/adr/adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md`. Reports based only on these known non-goals are out of scope:

- obfuscated shell input that requires full shell evaluation
- indirect execution where a safe write is followed by a later dangerous invocation
- runtime-generated commands such as `eval "$(…)"` payload assembly
- alias or shell-function expansion not visible in the raw intercepted command
- encoded payload variants outside the implemented heuristic coverage
- subshell or injection techniques that are not visible to the current interception boundary

## Disclosure expectations

- Please provide a reasonable time window for investigation and remediation before public disclosure.
- We will aim to acknowledge receipt promptly and keep you updated on triage status.
- If the report is accepted, we may credit the reporter unless you prefer to remain anonymous.
