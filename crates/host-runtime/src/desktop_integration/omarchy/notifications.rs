//! Persist at-most-once decisions before starting any desktop-session process.
use super::{Context, Error, files};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const SCHEMA: &str = "hiroute.omarchy-notifications/v1";
const MAX_SENT: usize = 65_536;
const MAX_PENDING: usize = 64;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionNotice {
    pub id: String,
    pub plan_name: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Notice {
    Suggestion {
        id: String,
        plan_name: String,
    },
    ServiceUnavailable,
    LaunchFailure {
        id: String,
        agent: String,
        reason: String,
    },
}
impl Notice {
    pub fn landing(&self) -> Option<String> {
        match self {
            Self::Suggestion { id, .. } => Some(format!("suggestion:{id}")),
            Self::LaunchFailure { agent, .. } => Some(format!("agent:{agent}")),
            Self::ServiceUnavailable => None,
        }
    }
    pub fn body(&self, zh: bool) -> String {
        let text = match self {
            Self::Suggestion { plan_name, .. } if zh => {
                format!("{plan_name} 有新的信任建议。打开查看依据。")
            }
            Self::Suggestion { plan_name, .. } => {
                format!("{plan_name} has a trust suggestion. Open its evidence.")
            }
            Self::ServiceUnavailable if zh => "HiRoute 服务暂时不可用。打开控制台检查。".into(),
            Self::ServiceUnavailable => {
                "The HiRoute service is unavailable. Open the console to inspect it.".into()
            }
            Self::LaunchFailure { agent, reason, .. } if zh => {
                format!("{agent} 路由启动检查未通过（{reason}）。请查看终端中的处理方式。")
            }
            Self::LaunchFailure { agent, reason, .. } => format!(
                "The routed {agent} launch check failed ({reason}). Review the terminal for recovery."
            ),
        };
        // Notification bodies may be parsed as Pango markup. Dynamic labels are text.
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    pub fn validate(&self) -> bool {
        match self {
            Self::Suggestion { id, plan_name } => {
                valid_id(id)
                    && plan_name.chars().count() <= 80
                    && !plan_name.chars().any(char::is_control)
            }
            Self::LaunchFailure { id, agent, reason } => {
                valid_id(id)
                    && matches!(agent.as_str(), "codex" | "claude-code")
                    && !reason.is_empty()
                    && reason.len() <= 80
                    && reason
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            }
            Self::ServiceUnavailable => true,
        }
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Store {
    schema: String,
    sent_suggestions: BTreeSet<String>,
    last_service_ready: Option<bool>,
    pending: Vec<Notice>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            schema: SCHEMA.into(),
            sent_suggestions: BTreeSet::new(),
            last_service_ready: None,
            pending: Vec::new(),
        }
    }
}
impl Context {
    fn notifications_path(&self) -> std::path::PathBuf {
        self.state.join("omarchy-notifications.v1.json")
    }
    fn read_notifications(&self) -> Result<Store, Error> {
        let Some((bytes, _)) = files::read(&self.notifications_path(), 0o600, 16 * 1024 * 1024)?
        else {
            return Ok(Store::default());
        };
        let value: Store = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRecord)?;
        if value.schema != SCHEMA
            || value.sent_suggestions.len() > MAX_SENT
            || value.pending.len() > MAX_PENDING
            || value.sent_suggestions.iter().any(|id| !valid_id(id))
            || value
                .pending
                .iter()
                .any(|n| !matches!(n, Notice::LaunchFailure { .. }) || !n.validate())
        {
            return Err(Error::InvalidRecord);
        }
        Ok(value)
    }
    fn save_notifications(&self, store: &Store) -> Result<(), Error> {
        files::write_private(
            &self.notifications_path(),
            &serde_json::to_vec(store).map_err(|_| Error::InvalidRecord)?,
        )
    }
    pub fn queue_launch_failure(&self, agent: &str, reason: &str) -> Result<(), Error> {
        let notice = Notice::LaunchFailure {
            id: files::nonce()?,
            agent: agent.into(),
            reason: reason.into(),
        };
        if !notice.validate() {
            return Err(Error::InvalidRecord);
        }
        let _lock = files::lock(self)?;
        let mut state = self.read_notifications()?;
        if !self.load()?.is_some_and(|m| m.enabled && m.notifications) {
            state.pending.clear();
            return self.save_notifications(&state);
        }
        if state.pending.len() >= MAX_PENDING {
            return Err(Error::NotificationCapacity);
        }
        state.pending.push(notice);
        self.save_notifications(&state)
    }
    /// Returns only newly due deliveries, after their state is durable. A failed spawn
    /// deliberately does not retry: at-most-once is stronger than eventual delivery.
    pub fn take_notifications(
        &self,
        ready: bool,
        suggestions: &[SuggestionNotice],
    ) -> Result<Vec<Notice>, Error> {
        if suggestions.len() > MAX_SENT
            || suggestions.iter().any(|s| {
                !valid_id(&s.id)
                    || s.plan_name.chars().count() > 80
                    || s.plan_name.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidRecord);
        }
        let _lock = files::lock(self)?;
        let mut state = self.read_notifications()?;
        if !self.load()?.is_some_and(|m| m.enabled && m.notifications) {
            state.pending.clear();
            state.last_service_ready = Some(ready);
            self.save_notifications(&state)?;
            return Ok(Vec::new());
        }
        let mut due = std::mem::take(&mut state.pending);
        if state.last_service_ready == Some(true) && !ready {
            due.push(Notice::ServiceUnavailable);
        }
        state.last_service_ready = Some(ready);
        if ready {
            for suggestion in suggestions {
                if state.sent_suggestions.contains(&suggestion.id) {
                    continue;
                }
                if state.sent_suggestions.len() >= MAX_SENT {
                    return Err(Error::NotificationCapacity);
                }
                state.sent_suggestions.insert(suggestion.id.clone());
                due.push(Notice::Suggestion {
                    id: suggestion.id.clone(),
                    plan_name: suggestion.plan_name.clone(),
                });
            }
        }
        self.save_notifications(&state)?;
        Ok(due)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{FakePlugin, enable, fixture};
    use super::*;
    #[test]
    fn omarchy_notifications_are_sent_once_across_reopen_and_state_transitions() {
        let (_temp, context, assets) = fixture();
        enable(&context, &assets, &FakePlugin::ready());
        let suggestions = [SuggestionNotice {
            id: "trust-stable".into(),
            plan_name: "<b>& Model".into(),
        }];
        assert_eq!(
            context
                .take_notifications(true, &suggestions)
                .unwrap()
                .len(),
            1
        );
        assert!(
            context
                .take_notifications(true, &suggestions)
                .unwrap()
                .is_empty()
        );
        assert_eq!(context.take_notifications(false, &[]).unwrap().len(), 1);
        assert!(context.take_notifications(false, &[]).unwrap().is_empty());
        context.take_notifications(true, &[]).unwrap();
        assert_eq!(context.take_notifications(false, &[]).unwrap().len(), 1);
        context
            .queue_launch_failure("codex", "service_unavailable")
            .unwrap();
        let pending = context.take_notifications(false, &[]).unwrap();
        assert!(matches!(&pending[..], [Notice::LaunchFailure { .. }]));
        assert!(context.take_notifications(false, &[]).unwrap().is_empty());
        context
            .queue_launch_failure("codex", "connection_unavailable")
            .unwrap();
        context.configure_notifications(false).unwrap();
        assert!(
            context
                .take_notifications(true, &suggestions)
                .unwrap()
                .is_empty()
        );
        context.configure_notifications(true).unwrap();
        assert!(
            context
                .take_notifications(true, &suggestions)
                .unwrap()
                .is_empty()
        );
        assert!(
            Notice::Suggestion {
                id: "id".into(),
                plan_name: "<b>& Model".into()
            }
            .body(false)
            .contains("&lt;b&gt;&amp; Model")
        );
    }
}
