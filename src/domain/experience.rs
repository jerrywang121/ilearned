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
