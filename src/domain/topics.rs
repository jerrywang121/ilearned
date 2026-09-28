use crate::error::AppError;

fn valid_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Canonical topic form: non-empty, `/`-separated segments of
/// `[a-z0-9_-]`; no leading/trailing `/`, no `//`, no `#` (hash is
/// pattern-only and never stored).
pub fn validate_topic(topic: &str) -> Result<(), AppError> {
    if topic.is_empty() {
        return Err(AppError::InvalidInput(
            "invalid topic '': topic must not be empty".to_string(),
        ));
    }
    if !topic.split('/').all(valid_segment) {
        return Err(AppError::InvalidInput(format!(
            "invalid topic '{topic}': segments must be [a-z0-9_-], '/' separated (e.g. travel/hotel/checkout)"
        )));
    }
    Ok(())
}

/// Topic pattern form: like [`validate_topic`] but a segment may be exactly
/// `#` (full-segment multi-level wildcard).
pub fn validate_topic_pattern(pattern: &str) -> Result<(), AppError> {
    if pattern.is_empty() {
        return Err(AppError::InvalidInput(
            "invalid topic pattern '': pattern must not be empty".to_string(),
        ));
    }
    if !pattern
        .split('/')
        .all(|seg| seg == "#" || valid_segment(seg))
    {
        return Err(AppError::InvalidInput(format!(
            "invalid topic pattern '{pattern}': segments must be [a-z0-9_-] or '#', '/' separated (e.g. travel/#)"
        )));
    }
    Ok(())
}

fn match_segments(pattern: &[&str], topic: &[&str]) -> bool {
    if pattern.is_empty() {
        return topic.is_empty();
    }
    if pattern[0] == "#" {
        // `#` matches zero or more levels.
        return (0..=topic.len()).any(|n| match_segments(&pattern[1..], &topic[n..]));
    }
    if topic.is_empty() || pattern[0] != topic[0] {
        return false;
    }
    match_segments(&pattern[1..], &topic[1..])
}

/// MQTT-style match where `#` matches zero or more levels and a bare
/// segment matches only itself (so bare `travel` never matches
/// `travel/hotel` — descendants require `travel/#`).
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    match_segments(
        &pattern.split('/').collect::<Vec<_>>(),
        &topic.split('/').collect::<Vec<_>>(),
    )
}

/// Keep the first `level` segments (`travel/hotel/checkout` at level 2 →
/// `travel/hotel`). Shorter topics are returned unchanged.
pub fn truncate_topic(topic: &str, level: u32) -> String {
    topic
        .split('/')
        .take(level as usize)
        .collect::<Vec<_>>()
        .join("/")
}
