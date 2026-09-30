use crate::domain::topics::validate_topic;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq)]
pub struct AddCommand {
    pub topic: String,
    pub when_text: String,
    pub if_text: String,
    pub do_text: String,
    pub check_text: String,
}

impl AddCommand {
    pub fn validate(&self) -> Result<(), AppError> {
        validate_topic(&self.topic)?;
        if self.when_text.trim().is_empty() {
            return Err(AppError::InvalidInput("when is required".to_string()));
        }
        if self.if_text.trim().is_empty() {
            return Err(AppError::InvalidInput("if is required".to_string()));
        }
        if self.do_text.trim().is_empty() {
            return Err(AppError::InvalidInput("do is required".to_string()));
        }
        if self.check_text.trim().is_empty() {
            return Err(AppError::InvalidInput("check is required".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpdateCommand {
    pub topic: String,
    pub id: String,
    pub when_text: Option<String>,
    pub if_text: Option<String>,
    pub do_text: Option<String>,
    pub check_text: Option<String>,
}

impl UpdateCommand {
    pub fn has_updates(&self) -> bool {
        self.when_text
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .if_text
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            || self
                .do_text
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            || self
                .check_text
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchQuery {
    pub topic: Option<String>,
    pub text: Option<String>,
    pub semantic: Option<String>,
    pub limit: u32,
    pub offset: u32,
    pub deep: bool,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            topic: None,
            text: None,
            semantic: None,
            limit: 20,
            offset: 0,
            deep: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackCommand {
    pub topic: String,
    pub id: String,
}

/// Topic listing/search input. `query=None`/blank lists all topics;
/// a query containing `#` is a multi-level wildcard pattern, otherwise a
/// substring. The query is lowercased before matching (topics are always
/// lowercase), so matching is case-insensitive. `level` truncates after
/// matching.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicQuery {
    pub query: Option<String>,
    pub level: Option<u32>,
    pub limit: u32,
    pub offset: u32,
    pub deep: bool,
}

impl Default for TopicQuery {
    fn default() -> Self {
        Self {
            query: None,
            level: None,
            limit: 20,
            offset: 0,
            deep: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClearCommand {
    Topic(String),
    All,
}

/// Counts of records transitioned to `deleted` by one clear operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearSummary {
    pub topics: u64,
    pub items: u64,
}
