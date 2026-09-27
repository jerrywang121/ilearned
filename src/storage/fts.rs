//! FTS5 query helpers: match-expression validation lives in the repository
//! (invalid syntax maps to `AppError::InvalidFtsSyntax`).

/// Returns true if `text` is blank (caller should browse instead of MATCH).
pub fn is_blank_query(text: &str) -> bool {
    text.trim().is_empty()
}
