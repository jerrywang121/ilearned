use super::experience::State;

#[derive(Debug, Clone, PartialEq)]
pub struct LifecycleConfig {
    pub active_period_days: u64,
    pub forget_period_days: u64,
    pub retention_days: u64,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            active_period_days: 60,
            forget_period_days: 120,
            retention_days: 60,
        }
    }
}

/// Search visibility: deleted/forgotten never eligible; inactive only when deep.
pub fn is_eligible(state: &State, deep: bool) -> bool {
    match state {
        State::Active => true,
        State::Inactive => deep,
        State::Deleted | State::Forgotten => false,
    }
}
