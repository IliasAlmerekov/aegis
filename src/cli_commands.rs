use std::env;
use std::path::PathBuf;

use aegis::audit::{AuditEntry, AuditIntegrityStatus, AuditLogger, AuditQuery};
use aegis::config::{Config, ValidationReport, validate_config_layers};
use aegis::error::AegisError;
use aegis::toggle;

use crate::{
    AuditArgs, AuditOutputFormat, ConfigArgs, ConfigCommand, ConfigValidateArgs,
    ConfigValidateOutput, EXIT_DENIED, EXIT_INTERNAL, RollbackArgs, rollback,
};

pub(crate) fn handle_audit_command(args: AuditArgs) -> i32 {
    if args.summary && matches!(args.format, AuditOutputFormat::Ndjson) {
        eprintln!("error: --summary cannot be used with --format ndjson");
        EXIT_DENIED
    } else if args.verify_integrity {
        if args.summary
            || args.last.is_some()
            || args.risk.is_some()
            || args.since.is_some()
            || args.until.is_some()
            || args.command_contains.is_some()
            || args.decision.is_some()
            || !matches!(args.format, AuditOutputFormat::Text)
        {
            eprintln!(
                "error: --verify-integrity cannot be combined with filters, --summary, or non-text formats"
            );
            EXIT_DENIED
        } else {
            let logger = AuditLogger::default();
            match logger.verify_integrity() {
                Ok(report) => {
                    println!("{}", report.message);
                    match report.status {
                        AuditIntegrityStatus::Verified => 0,
                        AuditIntegrityStatus::NoIntegrityData | AuditIntegrityStatus::Corrupt => {
                            EXIT_INTERNAL
                        }
                    }
                }
                Err(err) => {
                    eprintln!("error: failed to verify audit integrity: {err}");
                    EXIT_INTERNAL
                }
            }
        }
    } else {
        let logger = AuditLogger::default();
        let query = AuditQuery {
            last: args.last,
            risk: args.risk,
            decision: args.decision,
            since: args.since,
            until: args.until,
            command_contains: args.command_contains.clone(),
        };
        match logger.query(query) {
            Ok(entries) => match if args.summary {
                format_audit_summary(&entries, args.format)
            } else {
                format_audit_entries(&entries, args.format)
            } {
                Ok(output) => {
                    print!("{output}");
                    0
                }
                Err(err) => {
                    eprintln!("error: failed to serialize audit output: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => {
                eprintln!("error: failed to read audit log: {err}");
                EXIT_INTERNAL
            }
        }
    }
}

pub(crate) fn handle_config_command(args: ConfigArgs) -> i32 {
    match args.command {
        ConfigCommand::Init => match env::current_dir() {
            Ok(current_dir) => match Config::init_in(&current_dir) {
                Ok(path) => {
                    println!("{}", path.display());
                    0
                }
                Err(err) => {
                    eprintln!("error: failed to initialize config: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => {
                eprintln!("error: failed to resolve current directory: {err}");
                EXIT_INTERNAL
            }
        },
        ConfigCommand::Show => match Config::load_inspection() {
            Ok(config) => match config.to_toml_string() {
                Ok(toml) => {
                    print!("{toml}");
                    0
                }
                Err(err) => {
                    eprintln!("error: failed to serialize config: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => report_config_load_error(&err),
        },
        ConfigCommand::Validate(args) => handle_config_validate_command(args),
    }
}

pub(crate) fn handle_toggle_on_command() -> i32 {
    if let Err(err) = toggle::enable() {
        eprintln!("error: failed to enable Aegis: {err}");
        return EXIT_INTERNAL;
    }

    if let Err(err) = toggle::append_toggle_audit_entry("aegis on") {
        eprintln!("warning: toggle state changed, but audit entry could not be recorded: {err}");
    }

    println!("Aegis is enabled.");
    0
}

pub(crate) fn handle_toggle_off_command() -> i32 {
    if let Err(err) = toggle::disable() {
        eprintln!("error: failed to disable Aegis: {err}");
        return EXIT_INTERNAL;
    }

    if let Err(err) = toggle::append_toggle_audit_entry("aegis off") {
        eprintln!("warning: toggle state changed, but audit entry could not be recorded: {err}");
    }

    println!("Aegis is disabled until `aegis on`.");
    0
}

pub(crate) fn handle_toggle_status_command() -> i32 {
    let view = match toggle::status_view(aegis::runtime_gate::is_ci_environment()) {
        Ok(view) => view,
        Err(err) => {
            eprintln!("error: failed to read toggle status: {err}");
            return EXIT_INTERNAL;
        }
    };

    let toggle_label = match view.state {
        toggle::ToggleState::Disabled => "disabled",
        toggle::ToggleState::Enabled => "enabled",
    };

    println!("toggle: {toggle_label}");
    println!("flag: {}", view.flag_path.display());
    if view.ci_override_active && matches!(view.state, toggle::ToggleState::Disabled) {
        println!("effective mode: enforcing (CI override)");
    } else {
        println!(
            "effective mode: {}",
            if matches!(view.state, toggle::ToggleState::Disabled) {
                "disabled passthrough"
            } else {
                "enforcing"
            }
        );
    }
    println!("config: {}", view.config_status);
    0
}

pub(crate) fn handle_rollback_command(
    args: RollbackArgs,
    runtime: &tokio::runtime::Runtime,
) -> i32 {
    match runtime.block_on(rollback::execute(args.snapshot_id)) {
        Ok(target) => {
            println!(
                "rollback complete: plugin={} snapshot_id={}",
                target.plugin, target.snapshot_id
            );
            0
        }
        Err(err) if matches!(err, AegisError::Config(_)) => report_config_load_error(&err),
        Err(err) => {
            eprintln!("error: rollback failed: {err}");
            EXIT_INTERNAL
        }
    }
}

pub(crate) fn handle_config_validate_command(args: ConfigValidateArgs) -> i32 {
    let current_dir = match env::current_dir() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("error: failed to resolve current directory: {err}");
            return EXIT_INTERNAL;
        }
    };
    let home_dir = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let report = validate_config_layers(&current_dir, home_dir.as_deref());

    let render_result = match args.output {
        ConfigValidateOutput::Text => {
            print!("{}", format_validation_report_text(&report));
            Ok(())
        }
        ConfigValidateOutput::Json => serde_json::to_string_pretty(&report)
            .map(|json| {
                println!("{json}");
            })
            .map_err(|err| err.to_string()),
    };

    if let Err(err) = render_result {
        eprintln!("error: failed to serialize validation output: {err}");
        return EXIT_INTERNAL;
    }

    if report.errors.is_empty() {
        0
    } else {
        EXIT_INTERNAL
    }
}

pub(crate) fn format_validation_report_text(report: &ValidationReport) -> String {
    if report.errors.is_empty() && report.warnings.is_empty() {
        return "config is valid\n".to_string();
    }

    let mut out = String::new();

    if !report.errors.is_empty() {
        out.push_str("errors:\n");
        for issue in &report.errors {
            out.push_str(&format!(
                "- [{}] {}: {}\n",
                issue.code, issue.location, issue.message
            ));
        }
    }

    if !report.warnings.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("warnings:\n");
        for issue in &report.warnings {
            out.push_str(&format!(
                "- [{}] {}: {}\n",
                issue.code, issue.location, issue.message
            ));
        }
    }

    out
}

pub(crate) fn config_load_error_lines(err: &AegisError) -> Vec<String> {
    let mut lines = vec![format!("error: failed to load config: {err}")];

    if matches!(err, AegisError::Config(_)) {
        lines.push("error: Fix or remove the invalid config file and try again.".to_string());
    }

    lines
}

pub(crate) fn report_config_load_error(err: &AegisError) -> i32 {
    for line in config_load_error_lines(err) {
        eprintln!("{line}");
    }
    EXIT_INTERNAL
}

fn format_audit_entries(
    entries: &[AuditEntry],
    format: AuditOutputFormat,
) -> Result<String, String> {
    match format {
        AuditOutputFormat::Text => Ok(AuditLogger::format_entries(entries)),
        AuditOutputFormat::Json => {
            serde_json::to_string_pretty(entries).map_err(|err| err.to_string())
        }
        AuditOutputFormat::Ndjson => {
            let mut out = String::new();
            for entry in entries {
                let line = serde_json::to_string(entry).map_err(|err| err.to_string())?;
                out.push_str(&line);
                out.push('\n');
            }
            Ok(out)
        }
    }
}

fn format_audit_summary(
    entries: &[AuditEntry],
    format: AuditOutputFormat,
) -> Result<String, String> {
    let summary = AuditLogger::summarize_entries(entries);

    match format {
        AuditOutputFormat::Text => Ok(AuditLogger::format_summary(&summary)),
        AuditOutputFormat::Json => {
            serde_json::to_string_pretty(&summary).map_err(|err| err.to_string())
        }
        AuditOutputFormat::Ndjson => {
            Err("--summary cannot be used with --format ndjson".to_string())
        }
    }
}
