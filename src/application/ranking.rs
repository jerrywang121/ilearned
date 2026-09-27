//! Cosine similarity + reciprocal-rank fusion (k=60).

/// Cosine similarity in [-1, 1]; 0 when either vector is degenerate.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
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
