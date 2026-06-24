use std::io::Read;

use serde_json::Value;

use super::shell_quote;

/// Run the Claude Code `PreToolUse` hook and rewrite unwrapped Bash commands
/// through `aegis --command`.
pub(crate) fn run_hook() -> i32 {
    match hook_response_from_stdin() {
        HookOutcome::Allow(output) | HookOutcome::Deny(output) => {
            println!("{output}");
        }
        HookOutcome::Noop => {}
    }

    0
}

#[derive(Debug)]
pub(crate) enum HookOutcome {
    Allow(Value),
    Deny(Value),
    Noop,
}

fn hook_response_from_stdin() -> HookOutcome {
    let mut input = String::new();
    if let Err(err) = std::io::stdin().read_to_string(&mut input) {
        return HookOutcome::Deny(hook_deny_output(format!(
            "aegis could not read hook input: {err}"
        )));
    }

    hook_response_value(&input)
}

fn hook_response_value(input: &str) -> HookOutcome {
    let input: Value = match serde_json::from_str(input) {
        Ok(value) => value,
        Err(err) => {
            return HookOutcome::Deny(hook_deny_output(format!("invalid hook input: {err}")));
        }
    };

    let Some(root) = input.as_object() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: expected a JSON object".to_string(),
        ));
    };

    let Some(tool_input) = root.get("tool_input") else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: missing tool_input".to_string(),
        ));
    };

    let Some(tool_input) = tool_input.as_object() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: tool_input must be a JSON object".to_string(),
        ));
    };

    let Some(command_value) = tool_input.get("command") else {
        return HookOutcome::Noop;
    };

    let Some(command) = command_value.as_str() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: tool_input.command must be a string".to_string(),
        ));
    };

    // A command already in canonical wrapper form must pass through untouched —
    // re-wrapping would double-intercept. A command that merely begins with the
    // `aegis` word but is NOT a canonical wrapper is rejected: it could be a
    // half-formed or evasive wrapper, and wrapping it again would hide the
    // malformation. Fail closed with a clear reason instead of guessing.
    if is_canonical_aegis_wrapper(command) {
        return HookOutcome::Noop;
    }
    if starts_with_aegis_word(command) {
        return HookOutcome::Deny(hook_deny_output(
            "invalid aegis wrapper syntax; issue the command unwrapped and aegis will rewrite it"
                .to_string(),
        ));
    }

    let mut updated_input = tool_input.clone();
    updated_input.insert(
        "command".to_string(),
        Value::String(format!("aegis --command {}", shell_quote(command))),
    );

    HookOutcome::Allow(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "aegis intercept",
            "updatedInput": updated_input,
        }
    }))
}

fn hook_deny_output(reason: String) -> Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
}

/// The canonical command prefix Aegis rewrites Bash commands to.
const AEGIS_WRAPPER_PREFIX: &str = "aegis --command ";

/// True when `command` begins with the bare `aegis` executable word — either
/// exactly `aegis` or `aegis` followed by whitespace. Used to distinguish an
/// already-aegis invocation from an unrelated command like `aegisctl`.
fn starts_with_aegis_word(command: &str) -> bool {
    command
        .strip_prefix("aegis")
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
}

/// True only when `command` is exactly `aegis --command <arg>` where `<arg>` is
/// the POSIX single-quoted form `shell_quote` itself produces — i.e. re-quoting
/// the decoded argument reproduces the command byte-for-byte. This rejects
/// half-formed wrappers (`aegis --command '`) that merely share the prefix.
fn is_canonical_aegis_wrapper(command: &str) -> bool {
    let Some(payload) = command.strip_prefix(AEGIS_WRAPPER_PREFIX) else {
        return false;
    };
    match decode_single_quoted(payload) {
        Some(decoded) => shell_quote(&decoded) == payload,
        None => false,
    }
}

/// Decode a single POSIX single-quoted token of the exact shape `shell_quote`
/// emits: `'...'` with embedded single quotes encoded as the close-reopen
/// sequence `'\''`. Returns `None` for anything that is not one well-formed
/// single-quoted token (stray quotes, missing terminator, trailing content).
fn decode_single_quoted(payload: &str) -> Option<String> {
    let inner = payload.strip_prefix('\'')?;
    let chars: Vec<char> = inner.chars().collect();
    let mut decoded = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            // Final closing quote: must be the last character.
            if i == chars.len() - 1 {
                return Some(decoded);
            }
            // Otherwise the only legal continuation is the `'\''` escape.
            if chars.get(i + 1) == Some(&'\\')
                && chars.get(i + 2) == Some(&'\'')
                && chars.get(i + 3) == Some(&'\'')
            {
                decoded.push('\'');
                i += 4;
                continue;
            }
            return None;
        }
        decoded.push(chars[i]);
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_rewrites_plain_command_with_shell_quote() {
        let output =
            match hook_response_value(r#"{"tool_input":{"command":"git commit -m 'fix: hello'"}}"#)
            {
                HookOutcome::Allow(output) => output,
                other => panic!("expected rewrite output, got {other:?}"),
            };
        let rewritten = format!(
            "aegis --command {}",
            shell_quote("git commit -m 'fix: hello'")
        );

        let expected = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "permissionDecisionReason": "aegis intercept",
                "updatedInput": {
                    "command": rewritten
                }
            }
        });

        assert_eq!(output, expected);
    }

    #[test]
    fn hook_skips_already_wrapped_command() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"aegis --command 'rm -rf /tmp'"}}"#),
            HookOutcome::Noop
        ));
    }

    #[test]
    fn hook_skips_missing_command_field() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{}}"#),
            HookOutcome::Noop
        ));
    }

    #[test]
    fn hook_rejects_malformed_json_input() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"#),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_rejects_non_object_tool_input() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":"rm -rf /"}"#),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_does_not_skip_aegisctl_commands() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"aegisctl status"}}"#),
            HookOutcome::Allow(_)
        ));
    }

    #[test]
    fn hook_denies_non_canonical_aegis_wrapper() {
        // Begins with the `aegis` word but is not a canonical wrapper — must
        // fail closed rather than be re-wrapped or passed through.
        match hook_response_value(r#"{"tool_input":{"command":"aegis --command '"}}"#) {
            HookOutcome::Deny(output) => {
                assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
                assert!(
                    output["hookSpecificOutput"]["permissionDecisionReason"]
                        .as_str()
                        .unwrap()
                        .contains("invalid aegis wrapper syntax")
                );
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn hook_denies_bare_aegis_subcommand_that_is_not_canonical() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"aegis audit"}}"#),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_noops_canonical_wrapper_with_embedded_single_quotes() {
        // Round-trip: wrap a command containing single quotes, then confirm the
        // wrapper is recognized as canonical and passed through untouched.
        let wrapped = format!("aegis --command {}", shell_quote("echo 'oops'"));
        let input = serde_json::json!({ "tool_input": { "command": wrapped } }).to_string();
        assert!(matches!(hook_response_value(&input), HookOutcome::Noop));
    }

    #[test]
    fn is_canonical_aegis_wrapper_round_trips_arbitrary_commands() {
        for cmd in [
            "git status",
            "echo 'hi there'",
            "printf '%s\\n' 'a'\\''b'",
            "rm -rf /tmp/x",
        ] {
            let wrapped = format!("aegis --command {}", shell_quote(cmd));
            assert!(
                is_canonical_aegis_wrapper(&wrapped),
                "{wrapped:?} should be canonical"
            );
        }

        assert!(!is_canonical_aegis_wrapper("aegis --command "));
        assert!(!is_canonical_aegis_wrapper("aegis --command 'unterminated"));
        assert!(!is_canonical_aegis_wrapper("aegis --command 'a' extra"));
    }
}
