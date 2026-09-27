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
        if self.topic.trim().is_empty() {
            return Err(AppError::InvalidInput("topic is required".to_string()));
        }
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
pub struct ModifyCommand {
    pub topic: String,
    pub id: String,
    pub when_text: Option<String>,
    pub if_text: Option<String>,
    pub do_text: Option<String>,
    pub check_text: Option<String>,
}

impl ModifyCommand {
    pub fn has_updates(&self) -> bool {
        self.when_text.is_some()
            || self.if_text.is_some()
            || self.do_text.is_some()
            || self.check_text.is_some()
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

#[derive(Debug, Clone, PartialEq)]
pub enum ClearCommand {
    Topic(String),
    All,
}
