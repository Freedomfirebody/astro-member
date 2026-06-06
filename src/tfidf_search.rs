use std::collections::HashMap;

// A highly lightweight BM25-like search avoiding external Vector DBs
pub struct LightweightSearch;

impl LightweightSearch {
    pub fn score(query: &str, document: &str) -> f64 {
        let clean_word = |w: &str| -> String {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect()
        };

        let query_terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| clean_word(s))
            .filter(|s| !s.is_empty())
            .collect();

        let doc_terms: Vec<String> = document
            .to_lowercase()
            .split_whitespace()
            .map(|s| clean_word(s))
            .filter(|s| !s.is_empty())
            .collect();
        
        if query_terms.is_empty() || doc_terms.is_empty() {
            return 0.0;
        }

        let mut score = 0.0;
        let mut doc_freq = HashMap::new();
        for term in &doc_terms {
            *doc_freq.entry(term.clone()).or_insert(0) += 1;
        }

        let doc_len = doc_terms.len() as f64;

        // Simplified BM25 
        let k1 = 1.2;
        let b = 0.75;
        let avgdl = 20.0; // Assume avg length

        for term in &query_terms {
            if let Some(&freq) = doc_freq.get(term) {
                let tf = freq as f64;
                let idf = 1.5; // Stubbed IDF for completely local memory without global corpuses
                let numerator = tf * (k1 + 1.0);
                let denominator = tf + k1 * (1.0 - b + b * (doc_len / avgdl));
                score += idf * (numerator / denominator);
            }
        }

        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_punctuation_stripping() {
        // Query word "word" matches document word "word." or "word," or "!word!"
        let score1 = LightweightSearch::score("word", "word.");
        assert!(score1 > 0.0);

        let score2 = LightweightSearch::score("word", "word,");
        assert!(score2 > 0.0);

        let score3 = LightweightSearch::score("word", "!word!");
        assert!(score3 > 0.0);

        // Under normal case-insensitive search
        let score4 = LightweightSearch::score("Rust", "I write code in rust.");
        assert!(score4 > 0.0);

        // Keep hyphens and underscores
        let score5 = LightweightSearch::score("rust-mcp", "Using rust-mcp today.");
        assert!(score5 > 0.0);

        let score6 = LightweightSearch::score("session_id", "Isolated by session_id.");
        assert!(score6 > 0.0);
    }
}
