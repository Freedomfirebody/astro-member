use std::collections::HashMap;

// A highly lightweight BM25-like search avoiding external Vector DBs
pub struct LightweightSearch;

impl LightweightSearch {
    pub fn score(query: &str, document: &str) -> f64 {
        let query_terms: Vec<String> = query.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
        let doc_terms: Vec<String> = document.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
        
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
