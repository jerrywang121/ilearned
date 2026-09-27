use chrono::Utc;
use ilearned::domain::lifecycle::{is_eligible, LifecycleConfig};
use ilearned::domain::{AddCommand, Experience, ModifyCommand, State};
use ilearned::AppError;

fn valid_add() -> AddCommand {
    AddCommand {
        topic: "rust".to_string(),
        when_text: "when x".to_string(),
        if_text: "if y".to_string(),
        do_text: "do z".to_string(),
        check_text: "check w".to_string(),
    }
}

#[test]
fn add_rejects_empty_topic() {
    let mut cmd = valid_add();
    cmd.topic.clear();
    assert!(matches!(cmd.validate(), Err(AppError::InvalidInput(_))));

    let mut cmd = valid_add();
    cmd.when_text.clear();
    assert!(matches!(cmd.validate(), Err(AppError::InvalidInput(_))));

    let mut cmd = valid_add();
    cmd.if_text.clear();
    assert!(matches!(cmd.validate(), Err(AppError::InvalidInput(_))));

    let mut cmd = valid_add();
    cmd.do_text.clear();
    assert!(matches!(cmd.validate(), Err(AppError::InvalidInput(_))));

    let mut cmd = valid_add();
    cmd.check_text.clear();
    assert!(matches!(cmd.validate(), Err(AppError::InvalidInput(_))));

    assert!(valid_add().validate().is_ok());
}

#[test]
fn modify_rejects_no_fields() {
    let cmd = ModifyCommand {
        topic: "t".to_string(),
        id: "x".to_string(),
        when_text: None,
        if_text: None,
        do_text: None,
        check_text: None,
    };
    assert!(!cmd.has_updates());

    let cmd = ModifyCommand {
        topic: "t".to_string(),
        id: "x".to_string(),
        when_text: Some("w".to_string()),
        if_text: None,
        do_text: None,
        check_text: None,
    };
    assert!(cmd.has_updates());
}

#[test]
fn eligibility_rules() {
    assert!(is_eligible(&State::Active, false));
    assert!(is_eligible(&State::Active, true));
    assert!(!is_eligible(&State::Inactive, false));
    assert!(is_eligible(&State::Inactive, true));
    assert!(!is_eligible(&State::Deleted, false));
    assert!(!is_eligible(&State::Deleted, true));
    assert!(!is_eligible(&State::Forgotten, false));
    assert!(!is_eligible(&State::Forgotten, true));

    // LifecycleConfig defaults per spec: 60 / 120 / 60.
    let cfg = LifecycleConfig::default();
    assert_eq!(cfg.active_period_days, 60);
    assert_eq!(cfg.forget_period_days, 120);
    assert_eq!(cfg.retention_days, 60);
}

#[test]
fn experience_serde_uses_readme_names() {
    let e = Experience {
        topic: "t".to_string(),
        id: "abc123".to_string(),
        when_text: "w".to_string(),
        if_text: "i".to_string(),
        do_text: "d".to_string(),
        check_text: "c".to_string(),
        updated_at: Utc::now(),
        good_count: 1,
        bad_count: 0,
        state: State::Active,
    };
    let v = serde_json::to_value(&e).unwrap();
    assert!(v.get("when").is_some());
    assert!(v.get("if").is_some());
    assert!(v.get("do").is_some());
    assert!(v.get("check").is_some());
    assert!(v.get("when_text").is_none());
    assert!(v.get("if_text").is_none());
    assert!(v.get("do_text").is_none());
    assert!(v.get("check_text").is_none());
    assert_eq!(v.get("state").and_then(|s| s.as_str()), Some("active"));
}
