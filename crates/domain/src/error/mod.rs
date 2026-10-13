use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortErrorCode {
    Conflict,
    Corrupt,
    Crypto,
    InvalidData,
    NotFound,
    PermissionDenied,
    Unavailable,
}

/// Adapter errors carry a bounded context label and optional typed, non-secret capacity facts.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{code:?} at {context}")]
pub struct PortError {
    pub code: PortErrorCode,
    pub context: &'static str,
    pub model_budget_conflict: Option<Box<crate::AgentModelBudgetConflictV1>>,
}

impl PortError {
    pub const fn new(code: PortErrorCode, context: &'static str) -> Self {
        Self {
            code,
            context,
            model_budget_conflict: None,
        }
    }
    pub fn with_model_budget_conflict(
        mut self,
        conflict: crate::AgentModelBudgetConflictV1,
    ) -> Self {
        self.model_budget_conflict = Some(Box::new(conflict));
        self
    }
}

pub type PortResult<T> = Result<T, PortError>;
