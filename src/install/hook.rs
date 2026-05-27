use std::io::Read;

use serde_json::Value;

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

    if is_already_wrapped(command) {
        return HookOutcome::Noop;
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

fn is_already_wrapped(command: &str) -> bool {
    command
        .strip_prefix("aegis")
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
}

fn shell_quote(command: &str) -> String {
    format!("'{}'", command.replace('\'', "'\\''"))
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
            hook_response_value(
                r#"{"tool_input":{"command":#),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_rejects_non_object_tool_input() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":"rm -rf /"}"#
            ),
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
}
