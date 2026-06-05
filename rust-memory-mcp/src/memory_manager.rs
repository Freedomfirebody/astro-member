use crate::models::{MemoryEntry, MemoryLayer, SearchResult};
use crate::tfidf_search::LightweightSearch;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use chrono::Utc;

pub struct MemoryManager {
    base_dir: PathBuf,
    memories: Vec<MemoryEntry>,
}

impl MemoryManager {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let base_dir = path.as_ref().to_path_buf();
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
        }
        
        let mut manager = MemoryManager {
            base_dir,
            memories: Vec::new(),
        };
        
        manager.load_all()?;
        Ok(manager)
    }

    fn load_all(&mut self) -> Result<()> {
        self.memories.clear();
        for entry in fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().unwrap_or_default() == "json" {
                let content = fs::read_to_string(&path)?;
                if let Ok(memory) = serde_json::from_str::<MemoryEntry>(&content) {
                    self.memories.push(memory);
                }
            }
        }
        Ok(())
    }

    fn save_memory(&self, memory: &MemoryEntry) -> Result<()> {
        let file_path = self.base_dir.join(format!("{}.json", memory.id));
        let content = serde_json::to_string_pretty(memory)?;
        fs::write(file_path, content)?;
        Ok(())
    }

    pub fn store(&mut self, layer: MemoryLayer, session_id: Option<String>, content: String, tags: Vec<String>) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let memory = MemoryEntry {
            id: id.clone(),
            layer,
            session_id,
            content,
            tags,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0, // default neutral multiplier
        };

        self.save_memory(&memory)?;
        self.memories.push(memory);
        Ok(id)
    }

    pub fn retrieve(&mut self, query: &str, request_session_id: Option<String>) -> Result<Vec<SearchResult>> {
        let now = Utc::now();
        let mut results = Vec::new();

        for mem in self.memories.iter_mut() {
            // Memory Isolation: Rule 1 (If Session Layer, strictly isolate by SessionId)
            if mem.layer == MemoryLayer::Session {
                if mem.session_id != request_session_id {
                    continue;
                }
            }

            // Calculate textual relevance
            let text_score = LightweightSearch::score(query, &mem.content);
            if text_score == 0.0 { continue; }

            let days_old = (now - mem.created_at).num_days() as f64;
            
            // Memory Decay Mechanism
            let decay = (-mem.layer.decay_rate() * days_old).exp();
            
            // Layer weight + evaluation
            let layer_weight = mem.layer.base_weight();
            let mut final_score = text_score * layer_weight * decay * mem.evaluation_score;

            // Frequency boost
            let freq_boost = (1.0 + mem.access_count as f64).ln();
            final_score += freq_boost * 0.1;

            if final_score > 0.5 { // Threshold
                mem.access_count += 1;
                mem.last_accessed = now;
                self.save_memory(mem).ok(); // Optimistic save

                results.push(SearchResult {
                    memory: mem.clone(),
                    final_score,
                });
            }
        }

        // Sort by final unified activation score
        results.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap());
        // Return top 5 clustered context
        results.truncate(5);

        Ok(results)
    }

    pub fn evaluate_experience(&mut self, memory_id: &str, success: bool) -> Result<()> {
        if let Some(mem) = self.memories.iter_mut().find(|m| m.id == memory_id) {
            // Adaptive target adjustment based on goal completion
            if success {
                mem.evaluation_score *= 1.1; // Strengthen successful experiences
            } else {
                mem.evaluation_score *= 0.8; // Dampen failed experiences
            }
            // Cap at some bounds
            mem.evaluation_score = mem.evaluation_score.clamp(0.1, 5.0);
            self.save_memory(mem)?;
        }
        Ok(())
    }
}
