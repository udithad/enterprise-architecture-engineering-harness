//! Embedding abstraction (Phase 2, P2-D2).
//!
//! [`HashedNgramEmbedder`] is the built-in, deterministic, local embedder: it hashes
//! word unigrams/bigrams and character trigrams into a fixed-dimension signed vector
//! (feature hashing) and L2-normalises it. It runs in-process (no egress) and is
//! reproducible, but it is **lexical, not semantic**: it captures term and sub-word
//! overlap, not meaning. A semantic provider can be added behind [`EmbeddingProvider`].

use async_trait::async_trait;
use eaeh_core::{HarnessError, Result};

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Stable identifier recorded with every stored vector (e.g. `hashed-ngram-v1/384`).
    fn id(&self) -> &str;
    fn dims(&self) -> usize;
    fn egress(&self) -> crate::model::Egress {
        crate::model::Egress::External
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[derive(Debug, Clone)]
pub struct HashedNgramEmbedder {
    id: String,
    dims: usize,
}

impl HashedNgramEmbedder {
    pub fn new(dims: usize) -> Result<Self> {
        if !(32..=4096).contains(&dims) {
            return Err(HarnessError::invalid_input("embedding dims must be 32..=4096"));
        }
        Ok(Self {
            id: format!("hashed-ngram-v1/{dims}"),
            dims,
        })
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    fn add(&self, v: &mut [f32], feature: &str, weight: f32) {
        let h = Self::fnv1a(feature.as_bytes());
        let idx = (h % self.dims as u64) as usize;
        let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        v[idx] += sign * weight;
    }

    pub fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dims];
        let words: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase())
            .collect();
        for w in &words {
            self.add(&mut v, &format!("w:{w}"), 1.0);
            let chars: Vec<char> = format!("#{w}#").chars().collect();
            for tri in chars.windows(3) {
                self.add(&mut v, &format!("c:{}", tri.iter().collect::<String>()), 0.5);
            }
        }
        for pair in words.windows(2) {
            self.add(&mut v, &format!("b:{} {}", pair[0], pair[1]), 0.75);
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

#[async_trait]
impl EmbeddingProvider for HashedNgramEmbedder {
    fn id(&self) -> &str {
        &self.id
    }
    fn dims(&self) -> usize {
        self.dims
    }
    fn egress(&self) -> crate::model::Egress {
        crate::model::Egress::Local
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}

/// Cosine similarity of two equal-length vectors (0 when either is zero).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_bytes(b: &[u8]) -> Result<Vec<f32>> {
    if !b.len().is_multiple_of(4) {
        return Err(HarnessError::integrity(
            "embedding blob length not a multiple of 4",
        ));
    }
    Ok(b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_normalised_and_similarity_ordered() {
        let e = HashedNgramEmbedder::new(384).unwrap();
        let a = e.embed_one("Error responses must use problem+json with a trace id");
        assert_eq!(
            a,
            e.embed_one("Error responses must use problem+json with a trace id")
        );
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
        let near = e.embed_one("error response format problem json");
        let far = e.embed_one("database backup retention schedule");
        assert!(cosine(&a, &near) > cosine(&a, &far));
        assert_eq!(from_bytes(&to_bytes(&a)).unwrap(), a);
        assert!(HashedNgramEmbedder::new(8).is_err());
    }
}
