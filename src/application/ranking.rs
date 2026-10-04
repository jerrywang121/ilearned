//! Cosine similarity + reciprocal-rank fusion (k=60).

/// Cosine similarity in [-1, 1]; 0 when vectors are degenerate or differ in dimension.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na * nb).sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Fuse two ranked key lists with RRF: `score = Σ 1/(k+rank)`.
/// Rank is 1-based position in each list. Returns keys sorted by fused
/// score descending (ties broken by the caller).
pub fn rrf_fuse(
    text: &[(String, String)],
    sem: &[(String, String)],
    k: u32,
) -> Vec<((String, String), f32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<(String, String), f32> = HashMap::new();
    for (rank, key) in text.iter().enumerate() {
        *scores.entry(key.clone()).or_default() += 1.0 / (k as f32 + rank as f32 + 1.0);
    }
    for (rank, key) in sem.iter().enumerate() {
        *scores.entry(key.clone()).or_default() += 1.0 / (k as f32 + rank as f32 + 1.0);
    }
    let mut out: Vec<((String, String), f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out
}

pub const RRF_K: u32 = 60;

#[cfg(test)]
mod tests {
    use super::cosine;

    #[test]
    fn cosine_rejects_mismatched_dimensions() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cosine_returns_zero_for_empty_vectors() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[], &[1.0]), 0.0);
    }
}
