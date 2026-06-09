use std::borrow::Cow;

use aegis_types::RiskLevel;

use super::{Category, PatternSource, PatternToken, PrefixRule, s};
pub(super) fn rules() -> Vec<PrefixRule> {
    vec![
        // ── Docker ────────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("DK-001"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("system"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker system prune — removes all stopped containers, dangling images, unused networks, and build cache",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use '--filter until=24h' to limit pruning to older resources only",
            )),
            justification: Some(Cow::Borrowed(
                "Removes stopped containers, dangling images, networks, and build cache. Some of these may be needed for rollback or debugging.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker system prune -f"],
            not_match_examples: &["docker system info"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-002"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("volume"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker volume prune — removes all unused Docker volumes including any persistent data they hold",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List volumes first: 'docker volume ls' and back up data before pruning",
            )),
            justification: Some(Cow::Borrowed(
                "Deletes all unused volumes. If a volume is unmounted but contains important data, it will be lost permanently.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker volume prune -f"],
            not_match_examples: &["docker volume ls"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-003"),
            category: Category::Docker,
            pattern: vec![
                s("docker-compose"),
                s("down"),
                PatternToken::AnyStar,
                s("-v"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker-compose down -v — stops services and removes named volumes, deleting persistent data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Omit '-v' to keep volumes: 'docker-compose down' preserves volume data",
            )),
            justification: Some(Cow::Borrowed(
                "The -v flag removes named volumes, deleting persistent data that would otherwise survive container restarts.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker-compose down -v"],
            not_match_examples: &["docker-compose up"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-003"),
            category: Category::Docker,
            pattern: vec![
                s("docker"),
                s("compose"),
                s("down"),
                PatternToken::AnyStar,
                s("-v"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker compose down -v — stops services and removes named volumes, deleting persistent data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Omit '-v' to keep volumes: 'docker compose down' preserves volume data",
            )),
            justification: Some(Cow::Borrowed(
                "The -v flag removes named volumes, deleting persistent data that would otherwise survive container restarts.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker compose down -v"],
            not_match_examples: &["docker compose up"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-004"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("rmi")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker rmi — removes Docker images; rebuild time is lost if image is deleted unintentionally",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Tag images you want to keep before running bulk rmi commands",
            )),
            justification: Some(Cow::Borrowed(
                "Deleting images forces rebuilds and removes layers that other images may depend on.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker rmi my-image:latest"],
            not_match_examples: &["docker image ls"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-005"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("container"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker container prune — removes all stopped containers, including those with useful logs or data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Inspect stopped containers first: 'docker ps -a' before pruning",
            )),
            justification: Some(Cow::Borrowed(
                "Removes all stopped containers, including those with useful logs or forensic evidence.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker container prune -f"],
            not_match_examples: &["docker container ls"],
        },
        PrefixRule {
            id: Cow::Borrowed("DK-006"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("network"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker network prune — removes all unused Docker networks; can break containers that reconnect",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List networks in use: 'docker network ls' before pruning",
            )),
            justification: Some(Cow::Borrowed(
                "Removes networks that disconnected containers may still reference, causing reconnection failures.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["docker network prune -f"],
            not_match_examples: &["docker network ls"],
        },
        // ── Process ───────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("PS-001"),
            category: Category::Process,
            pattern: vec![
                s("kill"),
                PatternToken::Alts(vec![
                    Cow::Borrowed("-9"),
                    Cow::Borrowed("-KILL"),
                    Cow::Borrowed("-SIGKILL"),
                ]),
                s("1"),
            ],
            risk: RiskLevel::Block,
            description: Cow::Borrowed(
                "kill -9 1 — sends SIGKILL to PID 1 (init/systemd), immediately crashing the entire system",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'systemctl stop <service>' to stop individual services safely",
            )),
            justification: Some(Cow::Borrowed(
                "SIGKILL to PID 1 crashes the entire system immediately. There is no graceful shutdown of services or sync of data.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["kill -9 1"],
            not_match_examples: &["kill -15 1"],
        },
        PrefixRule {
            id: Cow::Borrowed("PS-002"),
            category: Category::Process,
            pattern: vec![s("pkill"), PatternToken::AnyStar, s("-9")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "pkill -9 — sends SIGKILL to all matching processes with no chance for cleanup or graceful shutdown",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'pkill -15' (SIGTERM) first to allow graceful shutdown before escalating",
            )),
            justification: Some(Cow::Borrowed(
                "SIGKILL prevents cleanup. Databases, editors, and services may leave corrupted files or lose unsaved work.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["pkill -9 nginx"],
            not_match_examples: &["pkill -15 nginx"],
        },
        PrefixRule {
            id: Cow::Borrowed("PS-005"),
            category: Category::Process,
            pattern: vec![s("chmod"), PatternToken::AnyStar, s("777"), s("/")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "chmod 777 / — makes the root filesystem world-writable, creating a severe security vulnerability",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Apply permissions only to the specific directory that needs them",
            )),
            justification: Some(Cow::Borrowed(
                "World-writable root allows any user to modify system binaries and config. This is a critical security vulnerability.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["chmod 777 /"],
            not_match_examples: &["chmod 755 /"],
        },
        // ── Package ───────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("PKG-005"),
            category: Category::Package,
            pattern: vec![
                s("pip"),
                s("install"),
                PatternToken::AnyStar,
                s("--trusted-host"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "pip install --trusted-host — disables SSL verification for the specified host",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Fix the SSL/TLS issue instead of bypassing verification; use a proper certificate",
            )),
            justification: Some(Cow::Borrowed(
                "Disables TLS certificate validation. An attacker on the network can inject malicious packages during install.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["pip install requests --trusted-host pypi.org"],
            not_match_examples: &["pip install requests"],
        },
    ]
}
