use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;
use zeroclaw_config::schema::CronShellOutputFormat;

pub fn deserialize_maybe_stringified<T: serde::de::DeserializeOwned>(
    v: &serde_json::Value,
) -> Result<T, serde_json::Error> {
    // Fast path: value is already the right shape (object, array, etc.)
    match serde_json::from_value::<T>(v.clone()) {
        Ok(parsed) => Ok(parsed),
        Err(first_err) => {
            // If it's a string, try parsing the string as JSON first.
            if let Some(s) = v.as_str() {
                let s = s.trim();
                if (s.starts_with('{') || s.starts_with('['))
                    && let Ok(inner) = serde_json::from_str::<serde_json::Value>(s)
                {
                    return serde_json::from_value::<T>(inner);
                }
            }
            Err(first_err)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    #[default]
    Shell,
    Agent,
}

impl From<JobType> for &'static str {
    fn from(value: JobType) -> Self {
        match value {
            JobType::Shell => "shell",
            JobType::Agent => "agent",
        }
    }
}

impl TryFrom<&str> for JobType {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "shell" => Ok(JobType::Shell),
            "agent" => Ok(JobType::Agent),
            _ => Err(format!(
                "Invalid job type '{}'. Expected one of: 'shell', 'agent'",
                value
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionTarget {
    #[default]
    Isolated,
    Main,
}

impl SessionTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Main => "main",
        }
    }

    /// Parse a session target, treating any non-`main` value (including typos)
    /// as isolated. Prefer [`Self::try_parse`] at user-input boundaries so an
    /// invalid value is rejected instead of silently becoming isolated.
    pub fn parse(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("main") {
            Self::Main
        } else {
            Self::Isolated
        }
    }

    /// Parse a session target from user input. `isolated` and `main` are
    /// accepted case-insensitively after trim; anything else is an error.
    pub fn try_parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "isolated" => Ok(Self::Isolated),
            "main" => Ok(Self::Main),
            _ => Err(format!(
                "Invalid session_target '{trimmed}'. Expected one of: 'isolated', 'main'"
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Schedule {
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
    },
    At {
        at: DateTime<Utc>,
    },
    Every {
        every_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default = "default_true")]
    pub best_effort: bool,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            mode: "none".to_string(),
            channel: None,
            to: None,
            thread_id: None,
            best_effort: true,
        }
    }
}

pub fn default_true() -> bool {
    true
}

fn default_source() -> String {
    "imperative".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CronOrigin {
    /// Channel composite that created the job (`<type>.<alias>`,
    /// e.g. `discord.main`). `None` for CLI/API/declarative jobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Platform guild/server ID (Discord `guild_id`). `None` for DMs and
    /// platforms without a guild concept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    /// Platform channel/conversation ID (Discord `channel_id`,
    /// Telegram `chat_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    /// Platform user ID that created the job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Originating message ID (`discord_<id>`, `telegram_<chat>_<msg>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl CronOrigin {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.channel.is_none()
            && self.guild_id.is_none()
            && self.channel_id.is_none()
            && self.user_id.is_none()
            && self.message_id.is_none()
    }
}

tokio::task_local! {
    static CRON_ORIGIN: Option<CronOrigin>;
}

/// Run `future` with `origin` visible to `cron_add` executed inline below.
/// Follows the `LIVE_SOP_ACTION_QUEUE` task-local pattern in `sop/executor.rs`:
/// per-message scope, never crosses `spawn!` boundaries, so background
/// scheduler runs (no scope) record `None`.
pub async fn scope_cron_origin<T>(
    origin: Option<CronOrigin>,
    future: impl Future<Output = T>,
) -> T {
    CRON_ORIGIN.scope(origin, future).await
}

/// Origin of the current chat turn, if any. `cron_add` calls this at
/// execution time — no signature churn on the `Tool` trait, no model-supplied
/// forgery (the model never sees this channel; it comes from the span scope).
#[must_use]
pub fn current_cron_origin() -> Option<CronOrigin> {
    CRON_ORIGIN.try_with(|o| o.clone()).unwrap_or(None)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub expression: String,
    pub schedule: Schedule,
    pub command: String,
    pub prompt: Option<String>,
    pub name: Option<String>,
    pub job_type: JobType,
    pub session_target: SessionTarget,
    pub model: Option<String>,
    /// Agent alias this job runs under. Empty when the row was written
    /// before the column existed and no agent has claimed it; the
    /// scheduler skips such rows with a warning rather than coercing
    /// them to a magic alias.
    #[serde(default)]
    pub agent_alias: String,
    pub enabled: bool,
    pub delivery: DeliveryConfig,
    pub delete_after_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Whether to recall and inject memory context before this agent job runs.
    /// Defaults to `true`; set to `false` for stateless digest jobs that should
    /// not accumulate or consume memory entries.
    #[serde(default = "default_true")]
    pub uses_memory: bool,
    /// How the job was created: `"imperative"` (CLI/API) or `"declarative"` (config).
    #[serde(default = "default_source")]
    pub source: String,
    /// Output format for shell jobs. `"wrapped"` (default) or `"raw"`.
    /// Declarative jobs read this from `CronJobDecl.shell_output_format` in
    /// the config; imperative jobs read it from the stored field in the DB.
    #[serde(default)]
    pub shell_output_format: CronShellOutputFormat,
    /// Where the job was created: channel composite + platform scope IDs +
    /// originating message. `None` for CLI/API/declarative jobs and rows
    /// written before the column existed. Immutable after creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<CronOrigin>,
    pub created_at: DateTime<Utc>,
    pub next_run: DateTime<Utc>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
    pub last_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRun {
    pub id: i64,
    pub job_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub status: String,
    pub output: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CronJobPatch {
    pub schedule: Option<Schedule>,
    pub command: Option<String>,
    pub prompt: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub delivery: Option<DeliveryConfig>,
    pub model: Option<String>,
    pub session_target: Option<SessionTarget>,
    pub delete_after_run: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub uses_memory: Option<bool>,
    pub shell_output_format: Option<CronShellOutputFormat>,
}

impl ::zeroclaw_api::attribution::Attributable for CronJob {
    fn role(&self) -> ::zeroclaw_api::attribution::Role {
        let kind = match self.schedule {
            Schedule::Cron { .. } => ::zeroclaw_api::attribution::CronKind::Cron,
            Schedule::At { .. } => ::zeroclaw_api::attribution::CronKind::At,
            Schedule::Every { .. } => ::zeroclaw_api::attribution::CronKind::Interval,
        };
        ::zeroclaw_api::attribution::Role::Cron(kind)
    }
    fn alias(&self) -> &str {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_schedule_from_object() {
        let val = serde_json::json!({"kind": "cron", "expr": "*/5 * * * *"});
        let sched = deserialize_maybe_stringified::<Schedule>(&val).unwrap();
        assert!(matches!(sched, Schedule::Cron { ref expr, .. } if expr == "*/5 * * * *"));
    }

    #[test]
    fn deserialize_schedule_from_string() {
        let val = serde_json::Value::String(r#"{"kind":"cron","expr":"*/5 * * * *"}"#.to_string());
        let sched = deserialize_maybe_stringified::<Schedule>(&val).unwrap();
        assert!(matches!(sched, Schedule::Cron { ref expr, .. } if expr == "*/5 * * * *"));
    }

    #[test]
    fn deserialize_schedule_string_with_tz() {
        let val = serde_json::Value::String(
            r#"{"kind":"cron","expr":"*/30 9-15 * * 1-5","tz":"Asia/Shanghai"}"#.to_string(),
        );
        let sched = deserialize_maybe_stringified::<Schedule>(&val).unwrap();
        match sched {
            Schedule::Cron { tz, .. } => assert_eq!(tz.as_deref(), Some("Asia/Shanghai")),
            _ => panic!("expected Cron variant"),
        }
    }

    #[test]
    fn deserialize_every_from_string() {
        let val = serde_json::Value::String(r#"{"kind":"every","every_ms":60000}"#.to_string());
        let sched = deserialize_maybe_stringified::<Schedule>(&val).unwrap();
        assert!(matches!(sched, Schedule::Every { every_ms: 60000 }));
    }

    #[test]
    fn deserialize_invalid_string_returns_error() {
        let val = serde_json::Value::String("not json at all".to_string());
        assert!(deserialize_maybe_stringified::<Schedule>(&val).is_err());
    }

    #[test]
    fn job_type_try_from_accepts_known_values_case_insensitive() {
        assert_eq!(JobType::try_from("shell").unwrap(), JobType::Shell);
        assert_eq!(JobType::try_from("SHELL").unwrap(), JobType::Shell);
        assert_eq!(JobType::try_from("agent").unwrap(), JobType::Agent);
        assert_eq!(JobType::try_from("AgEnT").unwrap(), JobType::Agent);
    }

    #[test]
    fn job_type_try_from_rejects_invalid_values() {
        assert!(JobType::try_from("").is_err());
        assert!(JobType::try_from("unknown").is_err());
    }

    #[test]
    fn session_target_try_parse_accepts_known_values_case_insensitive() {
        assert_eq!(
            SessionTarget::try_parse("isolated").unwrap(),
            SessionTarget::Isolated
        );
        assert_eq!(
            SessionTarget::try_parse("MAIN").unwrap(),
            SessionTarget::Main
        );
        assert_eq!(
            SessionTarget::try_parse("  main  ").unwrap(),
            SessionTarget::Main
        );
    }

    #[test]
    fn session_target_try_parse_rejects_empty_and_unknown_values() {
        assert!(SessionTarget::try_parse("").is_err());
        assert!(SessionTarget::try_parse("   ").is_err());
        let err = SessionTarget::try_parse("shared").unwrap_err();
        assert!(err.contains("session_target"));
        assert!(err.contains("isolated"));
        assert!(err.contains("main"));
    }

    #[test]
    fn session_target_parse_keeps_unknown_values_isolated_for_stored_rows() {
        assert_eq!(SessionTarget::parse("main"), SessionTarget::Main);
        assert_eq!(SessionTarget::parse("shared"), SessionTarget::Isolated);
        assert_eq!(SessionTarget::parse(""), SessionTarget::Isolated);
    }
}
