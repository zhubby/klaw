use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankedHit {
    pub id: String,
    pub title: String,
    pub score: f64,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FusedHit {
    pub id: String,
    pub title: String,
    pub score: f64,
    pub excerpt: String,
    pub lanes: Vec<String>,
}

pub fn reciprocal_rank_fuse(lanes: &[(&str, &[RankedHit])], k: usize) -> Vec<FusedHit> {
    let mut by_id: HashMap<String, FusedHit> = HashMap::new();

    for (lane_name, hits) in lanes {
        for (idx, hit) in hits.iter().enumerate() {
            let lane_score = 1.0 / (k as f64 + idx as f64 + 1.0);
            let fused = by_id.entry(hit.id.clone()).or_insert_with(|| FusedHit {
                id: hit.id.clone(),
                title: hit.title.clone(),
                score: 0.0,
                excerpt: hit.excerpt.clone(),
                lanes: Vec::new(),
            });
            fused.score += lane_score;
            if !fused.lanes.iter().any(|lane| lane == lane_name) {
                fused.lanes.push((*lane_name).to_string());
            }
        }
    }

    let mut results: Vec<FusedHit> = by_id.into_values().collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, score: f64) -> RankedHit {
        RankedHit {
            id: id.to_string(),
            title: id.to_string(),
            score,
            excerpt: format!("excerpt for {id}"),
        }
    }

    #[test]
    fn fuses_duplicate_ids_across_lanes() {
        let semantic = vec![hit("auth", 0.9), hit("cookie", 0.8)];
        let fts = vec![hit("auth", 0.7)];
        let fused = reciprocal_rank_fuse(&[("semantic", &semantic), ("fts", &fts)], 60);
        assert_eq!(fused[0].id, "auth");
        assert_eq!(fused[0].lanes.len(), 2);
    }

    #[test]
    fn keeps_unique_hits_when_only_one_lane_matches() {
        let semantic = vec![hit("auth", 0.9)];
        let fused = reciprocal_rank_fuse(&[("semantic", &semantic)], 60);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].lanes, vec!["semantic".to_string()]);
    }
}
