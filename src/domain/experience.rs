use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Active,
    Inactive,
    Deleted,
    Forgotten,
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            State::Active => "active",
            State::Inactive => "inactive",
            State::Deleted => "deleted",
            State::Forgotten => "forgotten",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Experience {
    pub topic: String,
    pub id: String,
    #[serde(rename = "when")]
    pub when_text: String,
    #[serde(rename = "if")]
    pub if_text: String,
    #[serde(rename = "do")]
    pub do_text: String,
    #[serde(rename = "check")]
    pub check_text: String,
    pub updated_at: DateTime<Utc>,
    pub good_count: u64,
    pub bad_count: u64,
    pub state: State,
}

impl Experience {
    /// Template-friendly lowercase state string (Askama calls methods).
    pub fn state_str(&self) -> &'static str {
        match self.state {
            State::Active => "active",
            State::Inactive => "inactive",
            State::Deleted => "deleted",
            State::Forgotten => "forgotten",
        }
    }
}
