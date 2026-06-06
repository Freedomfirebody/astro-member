use crate::models::{Association, MemoryEntry, MemoryLayer, MemoryStatus, SearchResult};
use crate::storage::SqliteStorage;
use crate::tfidf_search::LightweightSearch;
use anyhow::Result;
use chrono::Utc;
use std::path::Path;
use uuid::Uuid;

pub struct MemoryManager {
    pub storage: SqliteStorage,
    pub embedding_manager: crate::embedding::EmbeddingManager,
}

impl MemoryManager {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let db_path = if path_ref == Path::new(":memory:") {
            path_ref.to_path_buf()
        } else if path_ref.is_dir() || path_ref.extension().is_none() {
            let dir = path_ref;
            if !dir.exists() {
                std::fs::create_dir_all(dir)?;
            }
            dir.join("memory.db")
        } else {
            let parent = path_ref.parent();
            if let Some(p) = parent {
                if !p.exists() && !p.as_os_str().is_empty() {
                    std::fs::create_dir_all(p)?;
                }
            }
            path_ref.to_path_buf()
        };

        let cache_dir = if path_ref == Path::new(":memory:") {
            None
        } else if let Ok(env_cache) = std::env::var("FASTEMBED_CACHE_PATH") {
            if env_cache == "None" {
                None
            } else {
                Some(std::path::PathBuf::from(env_cache))
            }
        } else {
            db_path.parent().map(|p| p.join("models_cache"))
        };

        let storage = SqliteStorage::new(db_path)?;
        let embedding_manager = crate::embedding::EmbeddingManager::new(cache_dir);
        Ok(MemoryManager {
            storage,
            embedding_manager,
        })
    }

    pub fn store(
        &mut self,
        layer: MemoryLayer,
        session_id: Option<String>,
        content: String,
        tags: Vec<String>,
    ) -> Result<String> {
        if layer == MemoryLayer::Session {
            if session_id.is_none()
                || session_id
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            {
                return Err(anyhow::anyhow!(
                    "Session ID is required for Session layer memory"
                ));
            }
        } else {
            if session_id.is_some() {
                return Err(anyhow::anyhow!(
                    "Session ID must not be provided for non-Session layer memory"
                ));
            }
        }
        let id = Uuid::new_v4().to_string();
        let embedding = match self.embedding_manager.generate_passage_embedding(&content) {
            Ok(emb) => emb,
            Err(e) => {
                eprintln!("Warning: Failed to generate passage embedding: {:?}", e);
                Vec::new()
            }
        };
        let memory = MemoryEntry {
            id: id.clone(),
            layer,
            session_id,
            content,
            tags,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding,
            status: MemoryStatus::Active,
        };

        self.storage.insert_memory(&memory)?;
        Ok(id)
    }

    pub fn retrieve(
        &mut self,
        query: &str,
        request_session_id: Option<String>,
    ) -> Result<Vec<SearchResult>> {
        let now = Utc::now();

        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let query_embedding = match self.embedding_manager.generate_query_embedding(query) {
            Ok(emb) => emb,
            Err(e) => {
                eprintln!("Warning: Failed to generate query embedding: {:?}", e);
                Vec::new()
            }
        };
        let mut candidates = Vec::new();

        // Dynamically query only relevant memories from SQLite
        let memories = self
            .storage
            .get_relevant_memories(request_session_id.as_deref())?;

        for mem in memories {
            // Memory Isolation: Rule 1 (If Session Layer, strictly isolate by SessionId)
            if mem.layer == MemoryLayer::Session {
                if mem.session_id != request_session_id {
                    continue;
                }
            }

            // Calculate textual relevance
            let text_score = LightweightSearch::score(query, &mem.content);
            let normalized_text_score = text_score / (1.0 + text_score);

            // Calculate semantic relevance
            let semantic_score = if !query_embedding.is_empty() && !mem.embedding.is_empty() {
                let sim = crate::search::cosine_similarity(&query_embedding, &mem.embedding)
                    .unwrap_or(0.0);
                if sim < 0.65 {
                    0.0
                } else {
                    sim
                }
            } else {
                0.0
            };

            // Compute the combined relevance score using hybrid search
            let combined_relevance = normalized_text_score.max(semantic_score as f64);

            // Rules bypass the 0.15 relevance filter to remain permanently active
            if mem.layer != MemoryLayer::Rule && combined_relevance < 0.15 {
                continue;
            }

            // Calculate days_old as a fractional f64 value (using seconds old divided by 86400.0) based on last_accessed
            let seconds_old = ((now - mem.last_accessed).num_seconds() as f64).max(0.0);
            let days_old = seconds_old / 86400.0;

            // Memory Decay Mechanism
            let decay = (-mem.layer.decay_rate() * days_old).exp();

            // Make frequency boost multiplicative
            let freq_boost = 1.0 + (mem.access_count as f64 + 1.0).ln() * 0.1;

            // Layer weight + evaluation
            let layer_weight = mem.layer.base_weight();
            let final_score =
                combined_relevance * layer_weight * decay * mem.evaluation_score * freq_boost;

            candidates.push(SearchResult {
                memory: mem,
                final_score,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            });
        }

        // Sort by final unified activation score
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Return top 5 clustered context
        candidates.truncate(5);

        // Calculate metadata post-truncation
        let mut update_refs = Vec::new();
        let mut running_size = 0;
        for result in candidates.iter_mut() {
            result.memory.access_count = result.memory.access_count.saturating_add(1);
            result.memory.last_accessed = now;
            update_refs.push(&result.memory);

            result.size = result.memory.content.chars().count();
            running_size += result.size;
            result.cumulative_size = running_size;
            result.created_at = result.memory.created_at.to_rfc3339();
        }
        self.storage.update_memories_batch(&update_refs)?;

        Ok(candidates)
    }

    pub fn evaluate_experience(&mut self, memory_id: &str, success: bool) -> Result<()> {
        let mut mem = self
            .storage
            .get_memory_by_id(memory_id)?
            .ok_or_else(|| anyhow::anyhow!("Memory not found: {}", memory_id))?;

        if mem.layer != MemoryLayer::Experience {
            return Err(anyhow::anyhow!("Memory is not in the Experience layer"));
        }

        let engine =
            crate::evolution::EvolutionEngine::new(crate::evolution::EvolutionConfig::default());
        mem.evaluation_score = engine.evaluate(mem.evaluation_score, success, &mem.layer);
        self.storage.update_memory(&mem)?;
        Ok(())
    }

    pub fn get_memory_by_id(
        &self,
        id: &str,
        session_id: Option<&str>,
    ) -> Result<Option<MemoryEntry>> {
        if let Some(mem) = self.storage.get_memory_by_id(id)? {
            if mem.layer == MemoryLayer::Session {
                if let Some(sid) = session_id {
                    if mem.session_id.as_deref() == Some(sid) {
                        return Ok(Some(mem));
                    }
                }
                return Ok(None);
            }
            Ok(Some(mem))
        } else {
            Ok(None)
        }
    }

    pub fn get_conflict_candidates(
        &mut self,
        content: &str,
        session_id: Option<String>,
        threshold: Option<f64>,
        limit: Option<usize>,
    ) -> Result<Vec<crate::models::ConflictCandidate>> {
        let thresh = threshold.unwrap_or(0.70);
        let lim = limit.unwrap_or(5);

        let query_embedding = match self.embedding_manager.generate_query_embedding(content) {
            Ok(emb) => emb,
            Err(e) => {
                eprintln!("Warning: Failed to generate query embedding: {:?}", e);
                Vec::new()
            }
        };

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let memories = self.storage.get_relevant_memories(session_id.as_deref())?;
        let mut candidates = Vec::new();

        for mem in memories {
            if !mem.embedding.is_empty() {
                let sim = crate::search::cosine_similarity(&query_embedding, &mem.embedding)
                    .unwrap_or(0.0) as f64;
                if sim >= thresh {
                    candidates.push(crate::models::ConflictCandidate {
                        memory: mem,
                        similarity: sim,
                    });
                }
            }
        }

        candidates.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(lim);

        Ok(candidates)
    }

    pub fn resolve_conflict(
        &mut self,
        deprecate_ids: &[String],
        delete_ids: &[String],
        new_memories: &[crate::models::MemoryEntryInput],
        new_associations: &[crate::models::AssociationInput],
    ) -> Result<Vec<String>> {
        let mut inserted_ids = Vec::new();
        let mut entries = Vec::new();

        for input in new_memories {
            if input.layer == MemoryLayer::Session {
                if input.session_id.is_none()
                    || input
                        .session_id
                        .as_ref()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(true)
                {
                    return Err(anyhow::anyhow!(
                        "Session ID is required for Session layer memory"
                    ));
                }
            } else {
                if input.session_id.is_some() {
                    return Err(anyhow::anyhow!(
                        "Session ID must not be provided for non-Session layer memory"
                    ));
                }
            }

            let id = input
                .id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            inserted_ids.push(id.clone());

            let embedding = match self
                .embedding_manager
                .generate_passage_embedding(&input.content)
            {
                Ok(emb) => emb,
                Err(e) => {
                    eprintln!("Warning: Failed to generate passage embedding: {:?}", e);
                    Vec::new()
                }
            };

            let entry = MemoryEntry {
                id,
                layer: input.layer.clone(),
                session_id: input.session_id.clone(),
                content: input.content.clone(),
                tags: input.tags.clone(),
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 0,
                evaluation_score: 1.0,
                embedding,
                status: MemoryStatus::Active,
            };
            entries.push(entry);
        }

        let mut assocs = Vec::new();
        for assoc_input in new_associations {
            let rel_trimmed = assoc_input.relation_type.trim();
            if rel_trimmed.is_empty() {
                return Err(anyhow::anyhow!("Relation type cannot be empty"));
            }
            if assoc_input.source_id == assoc_input.target_id {
                return Err(anyhow::anyhow!(
                    "Self-referential associations are not allowed"
                ));
            }

            assocs.push(Association {
                source_id: assoc_input.source_id.clone(),
                target_id: assoc_input.target_id.clone(),
                relation_type: rel_trimmed.to_string(),
                created_at: Utc::now(),
            });
        }

        self.storage.execute_transactional_resolution(
            deprecate_ids,
            delete_ids,
            &entries,
            &assocs,
        )?;

        Ok(inserted_ids)
    }

    pub fn get_session_memories(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<MemoryEntry>> {
        if session_id.trim().is_empty() {
            return Err(anyhow::anyhow!("Session ID cannot be empty"));
        }
        self.storage.get_session_memories(session_id, limit)
    }

    pub fn purge_session_memories(
        &self,
        session_id: &str,
        preserve_ids: &[String],
        permanent: bool,
    ) -> Result<usize> {
        if session_id.trim().is_empty() {
            return Err(anyhow::anyhow!("Session ID cannot be empty"));
        }
        self.storage
            .purge_session_memories(session_id, preserve_ids, permanent)
    }

    pub fn create_association(
        &mut self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
    ) -> Result<()> {
        let relation_trimmed = relation_type.trim();
        if relation_trimmed.is_empty() {
            return Err(anyhow::anyhow!(
                "Relation type cannot be empty or whitespace-only"
            ));
        }
        if source_id == target_id {
            return Err(anyhow::anyhow!("Self-referential associations are not allowed (source_id and target_id are identical: {})", source_id));
        }

        // Validate existence of source and target nodes
        let source = self
            .storage
            .get_memory_by_id(source_id)?
            .ok_or_else(|| anyhow::anyhow!("Source memory with ID '{}' not found", source_id))?;
        let target = self
            .storage
            .get_memory_by_id(target_id)?
            .ok_or_else(|| anyhow::anyhow!("Target memory with ID '{}' not found", target_id))?;

        if source.layer == MemoryLayer::Session && target.layer == MemoryLayer::Session {
            if source.session_id.is_none()
                || target.session_id.is_none()
                || source.session_id != target.session_id
            {
                return Err(anyhow::anyhow!("Cross-session association is prohibited"));
            }
        }

        let assoc = Association {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            relation_type: relation_trimmed.to_string(),
            created_at: Utc::now(),
        };
        self.storage.create_association(&assoc)
    }

    pub fn get_associations(
        &mut self,
        source_id: &str,
        direction: Option<&str>,
    ) -> Result<Vec<Association>> {
        let dir = direction.unwrap_or("outgoing");
        if dir != "outgoing" && dir != "incoming" && dir != "both" {
            return Err(anyhow::anyhow!(
                "Invalid direction: '{}'. Allowed values are 'outgoing', 'incoming', or 'both'",
                dir
            ));
        }

        if self.storage.get_memory_by_id(source_id)?.is_none() {
            return Err(anyhow::anyhow!(
                "Source memory with ID '{}' not found",
                source_id
            ));
        }
        self.storage.get_associations(source_id, dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MemoryLayer;
    use std::sync::Arc;

    #[test]
    fn test_memory_manager_store_and_retrieve() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Store some memories
        let id1 = manager.store(
            MemoryLayer::Persona,
            None,
            "Always be concise and helpful.".to_string(),
            vec!["behavior".to_string()],
        )?;
        let id2 = manager.store(
            MemoryLayer::Session,
            Some("session-123".to_string()),
            "User prefers Python over Rust.".to_string(),
            vec!["preferences".to_string()],
        )?;

        // Retrieve
        let results = manager.retrieve("concise", None)?;
        println!("DEBUG RESULTS: {:#?}", results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.id, id1);

        // Retrieve session memory with correct session ID
        let results_session =
            manager.retrieve("prefers Python", Some("session-123".to_string()))?;
        assert_eq!(results_session.len(), 1);
        assert_eq!(results_session[0].memory.id, id2);

        // Retrieve session memory with incorrect session ID (should be isolated)
        let results_wrong_session =
            manager.retrieve("prefers Python", Some("session-456".to_string()))?;
        assert!(results_wrong_session.is_empty());

        // Retrieve session memory with no session ID (should be isolated)
        let results_no_session = manager.retrieve("prefers Python", None)?;
        assert!(results_no_session.is_empty());

        Ok(())
    }

    #[test]
    fn test_edge_case_empty_inputs() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // 1. Empty retrieve query
        let results = manager.retrieve("", None)?;
        assert!(results.is_empty(), "Empty query should return no results");

        let results_spaces = manager.retrieve("   ", None)?;
        assert!(
            results_spaces.is_empty(),
            "Whitespace-only query should return no results"
        );

        // 2. Empty content store
        let _id = manager.store(MemoryLayer::Experience, None, "".to_string(), vec![])?;
        // Storing empty content is allowed by the database/code.
        // But searching for it should return nothing since BM25 score will be 0.
        let results = manager.retrieve("", None)?;
        assert!(results.is_empty());
        let results_any = manager.retrieve("anything", None)?;
        assert!(results_any.is_empty());

        Ok(())
    }

    #[test]
    fn test_edge_case_session_without_id() {
        let mut manager = MemoryManager::new(":memory:").unwrap();
        // Storing a Session layer memory without session_id should return an error.
        let res = manager.store(
            MemoryLayer::Session,
            None,
            "Some session memory".to_string(),
            vec![],
        );
        assert!(
            res.is_err(),
            "Session layer memory must require a session ID"
        );
    }

    #[test]
    fn test_edge_case_session_with_empty_id() {
        let mut manager = MemoryManager::new(":memory:").unwrap();
        // Storing a Session layer memory with empty or whitespace-only session_id should return an error.
        let res1 = manager.store(
            MemoryLayer::Session,
            Some("".to_string()),
            "Some session memory".to_string(),
            vec![],
        );
        assert!(res1.is_err());
        assert_eq!(
            res1.unwrap_err().to_string(),
            "Session ID is required for Session layer memory"
        );

        let res2 = manager.store(
            MemoryLayer::Session,
            Some("   ".to_string()),
            "Some session memory".to_string(),
            vec![],
        );
        assert!(res2.is_err());
        assert_eq!(
            res2.unwrap_err().to_string(),
            "Session ID is required for Session layer memory"
        );
    }

    #[test]
    fn test_edge_case_non_session_with_session_id() {
        let mut manager = MemoryManager::new(":memory:").unwrap();
        // Storing a non-Session layer memory with a session ID should return an error.
        let res = manager.store(
            MemoryLayer::Rule,
            Some("session-123".to_string()),
            "Some principle memory".to_string(),
            vec![],
        );
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "Session ID must not be provided for non-Session layer memory"
        );
    }

    #[test]
    fn test_edge_case_large_text() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Construct a very large text entry (approx. 50,000 characters)
        let mut large_content = "start ".to_string();
        for _ in 0..10000 {
            large_content.push_str("word ");
        }
        large_content.push_str("unique_token end");

        let id = manager.store(
            MemoryLayer::Rule,
            None,
            large_content,
            vec!["large".to_string()],
        )?;

        // Retrieve using the unique token
        let results = manager.retrieve("unique_token", None)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.id, id);

        Ok(())
    }

    #[test]
    fn test_edge_case_orphaned_associations() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let source = "non-existent-source-id";
        let target = "non-existent-target-id";
        let rel = "relates_to";

        // Creating association between non-existent IDs should fail now due to foreign keys
        let res = manager.create_association(source, target, rel);
        assert!(
            res.is_err(),
            "Should fail to create association between non-existent memory IDs"
        );
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Source memory with ID 'non-existent-source-id' not found"));

        // Now store them so they exist
        let id_src = manager.store(
            MemoryLayer::Rule,
            None,
            "Source principle content".to_string(),
            vec![],
        )?;
        let id_tgt = manager.store(
            MemoryLayer::Rule,
            None,
            "Target principle content".to_string(),
            vec![],
        )?;

        // Creating association with target missing should also fail
        let res_tgt_missing = manager.create_association(&id_src, "non-existent-target", rel);
        assert!(res_tgt_missing.is_err());
        assert!(res_tgt_missing
            .unwrap_err()
            .to_string()
            .contains("Target memory with ID 'non-existent-target' not found"));

        manager.create_association(&id_src, &id_tgt, rel)?;

        // Retrieve associations for the source
        let assocs = manager.get_associations(&id_src, None)?;
        assert_eq!(assocs.len(), 1);
        assert_eq!(assocs[0].source_id, id_src);
        assert_eq!(assocs[0].target_id, id_tgt);
        assert_eq!(assocs[0].relation_type, rel);

        // Querying associations for a non-existent source should fail
        let res_query_missing = manager.get_associations("non-existent-source-id", None);
        assert!(res_query_missing.is_err());
        assert!(res_query_missing
            .unwrap_err()
            .to_string()
            .contains("Source memory with ID 'non-existent-source-id' not found"));

        Ok(())
    }

    #[test]
    fn test_cross_session_associations() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let id_s1 = manager.store(
            MemoryLayer::Session,
            Some("session-1".to_string()),
            "Session 1 content".to_string(),
            vec![],
        )?;
        let id_s2 = manager.store(
            MemoryLayer::Session,
            Some("session-2".to_string()),
            "Session 2 content".to_string(),
            vec![],
        )?;
        let id_s1_b = manager.store(
            MemoryLayer::Session,
            Some("session-1".to_string()),
            "Session 1 other content".to_string(),
            vec![],
        )?;

        // Associating same session memories should succeed
        manager.create_association(&id_s1, &id_s1_b, "related")?;

        // Associating different session memories should fail
        let res = manager.create_association(&id_s1, &id_s2, "related");
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Cross-session association is prohibited"));

        Ok(())
    }

    #[test]
    fn test_invalid_relation_type_and_self_associations() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let id_src = manager.store(MemoryLayer::Rule, None, "Source node".to_string(), vec![])?;
        let id_tgt = manager.store(MemoryLayer::Rule, None, "Target node".to_string(), vec![])?;

        // 1. Empty relation_type
        let res_empty = manager.create_association(&id_src, &id_tgt, "");
        assert!(res_empty.is_err());
        assert_eq!(
            res_empty.unwrap_err().to_string(),
            "Relation type cannot be empty or whitespace-only"
        );

        // 2. Whitespace-only relation_type
        let res_space = manager.create_association(&id_src, &id_tgt, "   ");
        assert!(res_space.is_err());
        assert_eq!(
            res_space.unwrap_err().to_string(),
            "Relation type cannot be empty or whitespace-only"
        );

        // 3. Self-referential association
        let res_self = manager.create_association(&id_src, &id_src, "depends_on");
        assert!(res_self.is_err());
        assert!(res_self
            .unwrap_err()
            .to_string()
            .contains("Self-referential associations are not allowed"));

        Ok(())
    }

    #[test]
    fn test_bidirectional_associations() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let id_a = manager.store(MemoryLayer::Rule, None, "Node A".to_string(), vec![])?;
        let id_b = manager.store(MemoryLayer::Rule, None, "Node B".to_string(), vec![])?;
        let id_c = manager.store(MemoryLayer::Rule, None, "Node C".to_string(), vec![])?;

        // A -> B
        manager.create_association(&id_a, &id_b, "links_to")?;
        // C -> A
        manager.create_association(&id_c, &id_a, "depends_on")?;

        // Query outgoing from A (A is source) -> should return A -> B
        let outgoing = manager.get_associations(&id_a, Some("outgoing"))?;
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].target_id, id_b);
        assert_eq!(outgoing[0].relation_type, "links_to");

        // Query incoming to A (A is target) -> should return C -> A
        let incoming = manager.get_associations(&id_a, Some("incoming"))?;
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source_id, id_c);
        assert_eq!(incoming[0].relation_type, "depends_on");

        // Query both for A -> should return both
        let both = manager.get_associations(&id_a, Some("both"))?;
        assert_eq!(both.len(), 2);
        assert!(both
            .iter()
            .any(|assoc| assoc.source_id == id_a && assoc.target_id == id_b));
        assert!(both
            .iter()
            .any(|assoc| assoc.source_id == id_c && assoc.target_id == id_a));

        // Invalid direction parameter should fail
        let err_dir = manager.get_associations(&id_a, Some("invalid_dir"));
        assert!(err_dir.is_err());

        Ok(())
    }

    #[test]
    fn test_get_memory_by_id_feature() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;
        let id = manager.store(
            MemoryLayer::Rule,
            None,
            "Testing get_memory_by_id".to_string(),
            vec![],
        )?;

        let retrieved = manager.get_memory_by_id(&id, None)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "Testing get_memory_by_id");

        let not_found = manager.get_memory_by_id("non-existent-id", None)?;
        assert!(not_found.is_none());

        // Test session isolation in get_memory_by_id
        let session_id = "session-456".to_string();
        let s_id = manager.store(
            MemoryLayer::Session,
            Some(session_id.clone()),
            "Session specific content".to_string(),
            vec![],
        )?;

        // Retrieving with correct session ID should succeed
        let retrieved_s = manager.get_memory_by_id(&s_id, Some(&session_id))?;
        assert!(retrieved_s.is_some());
        assert_eq!(retrieved_s.unwrap().content, "Session specific content");

        // Retrieving with incorrect session ID should return None
        let retrieved_wrong = manager.get_memory_by_id(&s_id, Some("wrong-session"))?;
        assert!(retrieved_wrong.is_none());

        // Retrieving with None session ID should return None
        let retrieved_none = manager.get_memory_by_id(&s_id, None)?;
        assert!(retrieved_none.is_none());

        Ok(())
    }

    #[test]
    fn test_experience_adaptation_and_clamping() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let id = manager.store(
            MemoryLayer::Experience,
            None,
            "Debugging memory issues".to_string(),
            vec![],
        )?;

        // Initial score should be 1.0 (retrieved via get_memory_by_id)
        let initial_entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(initial_entry.evaluation_score, 1.0);

        // Evaluate success (should multiply by 1.1)
        manager.evaluate_experience(&id, true)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert!((entry.evaluation_score - 1.1).abs() < 1e-9);

        // Evaluate success multiple times to trigger clamp at 5.0
        for _ in 0..20 {
            manager.evaluate_experience(&id, true)?;
        }
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 5.0);

        // Evaluate failure multiple times to trigger clamp at 0.1
        for _ in 0..30 {
            manager.evaluate_experience(&id, false)?;
        }
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 0.1);

        // Verify error when evaluating non-Experience memory or non-existent memory
        let id_rule =
            manager.store(MemoryLayer::Rule, None, "A rule memory".to_string(), vec![])?;
        assert!(manager.evaluate_experience(&id_rule, true).is_err());
        assert!(manager
            .evaluate_experience("non-existent-id", true)
            .is_err());

        Ok(())
    }

    #[test]
    fn test_memory_decay() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Store a session memory
        let content = "Learning Rust macros today.".to_string();
        let id_fresh = manager.store(
            MemoryLayer::Session,
            Some("session-1".to_string()),
            content.clone(),
            vec![],
        )?;

        // Retrieve fresh to get score
        let fresh_results = manager.retrieve("macros", Some("session-1".to_string()))?;
        assert_eq!(fresh_results.len(), 1);

        // Store a second session memory with the same content
        let id_decayed = manager.store(
            MemoryLayer::Session,
            Some("session-1".to_string()),
            content.clone(),
            vec![],
        )?;

        // Force access count back to 0 for a fair comparison since retrieve increments it
        // and manually update its last_accessed in the database to be 2 days ago.
        let mut entry = manager.storage.get_memory_by_id(&id_decayed)?.unwrap();
        entry.last_accessed = Utc::now() - chrono::Duration::days(2);
        entry.access_count = 0; // reset
        manager.storage.update_memory(&entry)?;

        // Reset fresh memory access count to 0 in storage to compare fairly
        let mut fresh_entry = manager.storage.get_memory_by_id(&id_fresh)?.unwrap();
        fresh_entry.access_count = 0;
        manager.storage.update_memory(&fresh_entry)?;

        // Retrieve again
        let results = manager.retrieve("macros", Some("session-1".to_string()))?;
        // Both should match, but fresh should have a higher score than decayed due to decay factor
        assert_eq!(results.len(), 2);

        let fresh_retrieved = results.iter().find(|r| r.memory.id == id_fresh).unwrap();
        let decayed_retrieved = results.iter().find(|r| r.memory.id == id_decayed).unwrap();

        assert!(
            fresh_retrieved.final_score > decayed_retrieved.final_score,
            "Fresh score {} should be greater than decayed score {}",
            fresh_retrieved.final_score,
            decayed_retrieved.final_score
        );

        Ok(())
    }

    #[test]
    fn test_semantic_similarity_synonyms() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Store memory with python preference
        let id = manager.store(
            MemoryLayer::Experience,
            None,
            "The developer prefers utilizing Python for programming tasks.".to_string(),
            vec!["dev".to_string()],
        )?;

        // Query with synonyms, no overlapping words
        let results = manager.retrieve("coder software construction language preference", None)?;

        let query_emb = manager
            .embedding_manager
            .generate_query_embedding("test")
            .unwrap_or_default();
        if !query_emb.is_empty() {
            assert!(!results.is_empty(), "Should retrieve memory via semantic search even with synonyms when embedding model is loaded");
            assert_eq!(results[0].memory.id, id);
        } else {
            // If fastembed couldn't initialize (e.g. no internet/offline caching), it should return empty results
            assert!(
                results.is_empty(),
                "Results should be empty when embedding generation fails/returns empty"
            );
        }

        Ok(())
    }

    #[test]
    fn test_hybrid_search_combination() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Memory A has exact keyword matches but less semantic alignment for a general concept
        let _id_a = manager.store(
            MemoryLayer::Rule,
            None,
            "banana apple fruit orange".to_string(),
            vec!["fruits".to_string()],
        )?;

        // Memory B has semantic alignment for the query but no direct keyword overlaps
        let id_b = manager.store(
            MemoryLayer::Rule,
            None,
            "A canine is barking loudly in the yard.".to_string(),
            vec!["animals".to_string()],
        )?;

        // Query: "loud dog barking outside"
        // Dog -> canine, outside -> yard. Barking is a keyword match for both.
        // Let's verify we retrieve both correctly.
        let results = manager.retrieve("barking dog yard", None)?;
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.memory.id == id_b));

        Ok(())
    }

    #[test]
    fn test_float_sorting_nan_safety() {
        let mut candidates = vec![
            SearchResult {
                memory: MemoryEntry {
                    id: "1".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "a".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: std::f64::NAN,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
            SearchResult {
                memory: MemoryEntry {
                    id: "2".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "b".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: 1.5,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
            SearchResult {
                memory: MemoryEntry {
                    id: "3".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "c".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: 2.0,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
        ];

        // This should not panic
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Ensure we can sort without panics.
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn test_float_sorting_stress() {
        let special_scores = [
            std::f64::NAN,
            std::f64::INFINITY,
            std::f64::NEG_INFINITY,
            0.0,
            -0.0,
            1e-300,
            1e300,
            1.5,
            -1.5,
        ];

        let mut candidates = Vec::new();
        for (i, &score) in special_scores.iter().enumerate() {
            candidates.push(SearchResult {
                memory: MemoryEntry {
                    id: i.to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "test".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: score,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            });
        }

        // Run sort in both directions to verify it never panics
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(candidates.len(), special_scores.len());

        candidates.sort_by(|a, b| {
            a.final_score
                .partial_cmp(&b.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(candidates.len(), special_scores.len());
    }

    #[test]
    fn test_embedding_failure_graceful_fallback() -> Result<()> {
        // Create a manager with an invalid cache directory (e.g. pointing to a file)
        // to force fastembed initialization to fail.
        let invalid_cache_dir = std::path::PathBuf::from("Cargo.toml"); // this is a file, not a directory

        let storage = SqliteStorage::new(":memory:")?;
        let manager = MemoryManager {
            storage,
            embedding_manager: crate::embedding::EmbeddingManager::new(Some(invalid_cache_dir)),
        };

        // Storing a memory should still succeed and print a warning to stderr
        let mut manager = manager;
        let memory_id = manager.store(
            MemoryLayer::Rule,
            None,
            "Graceful fallback test".to_string(),
            vec![],
        )?;

        // The stored memory's embedding should be empty
        let stored = manager.storage.get_memory_by_id(&memory_id)?.unwrap();
        assert!(
            stored.embedding.is_empty(),
            "Embedding must be empty on failure"
        );

        // Retrieval should also succeed and return the stored memory based on text matching
        let results = manager.retrieve("Graceful fallback", None)?;
        assert_eq!(results.len(), 1, "Should retrieve via text-search");
        assert_eq!(results[0].memory.id, memory_id);

        Ok(())
    }

    #[test]
    fn test_rule_relevance_exemption() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // 1. Store a Rule memory
        let rule_id = manager.store(
            MemoryLayer::Rule,
            None,
            "Always use strict formatting rules.".to_string(),
            vec![],
        )?;

        // 2. Store a Session memory
        let session_id = manager.store(
            MemoryLayer::Session,
            Some("session-123".to_string()),
            "Always use strict formatting rules.".to_string(),
            vec![],
        )?;

        // Query with an irrelevant query (should result in relevance score close to 0)
        // Rule should bypass the 0.15 relevance filter, but Session memory should be filtered out.
        let results = manager.retrieve("banana apple orange", Some("session-123".to_string()))?;

        // Assert Rule memory is retrieved
        let rule_retrieved = results.iter().any(|r| r.memory.id == rule_id);
        assert!(
            rule_retrieved,
            "Rule memory should be retrieved despite low relevance"
        );

        // Assert Session memory is filtered out
        let session_retrieved = results.iter().any(|r| r.memory.id == session_id);
        assert!(
            !session_retrieved,
            "Session memory should be filtered out due to low relevance (< 0.15)"
        );

        Ok(())
    }

    #[test]
    fn test_freq_boost_zero_access_count() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Store a memory
        let _id = manager.store(
            MemoryLayer::Rule,
            None,
            "Verify frequency boost behavior.".to_string(),
            vec![],
        )?;

        // Retrieve it for the first time when its access_count is 0 in the database
        let results = manager.retrieve("frequency boost behavior", None)?;
        assert_eq!(results.len(), 1);

        // Verify that the final score is finite and greater than zero (no longer -inf due to log(0) bug)
        assert!(
            results[0].final_score.is_finite(),
            "Initial score for 0 access count should be finite"
        );
        assert!(
            results[0].final_score > 0.0,
            "Initial score for 0 access count should be greater than 0"
        );

        Ok(())
    }

    #[test]
    fn test_adversarial_graph_associations() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // Store two memories
        let id_a = manager.store(
            MemoryLayer::Rule,
            None,
            "Node A content".to_string(),
            vec![],
        )?;
        let id_b = manager.store(
            MemoryLayer::Rule,
            None,
            "Node B content".to_string(),
            vec![],
        )?;

        // 1. Whitespace relation types
        let res_empty = manager.create_association(&id_a, &id_b, "");
        assert!(res_empty.is_err());
        assert_eq!(
            res_empty.unwrap_err().to_string(),
            "Relation type cannot be empty or whitespace-only"
        );

        let res_whitespace = manager.create_association(&id_a, &id_b, "   ");
        assert!(res_whitespace.is_err());
        assert_eq!(
            res_whitespace.unwrap_err().to_string(),
            "Relation type cannot be empty or whitespace-only"
        );

        let res_tab_newline = manager.create_association(&id_a, &id_b, "\n\t");
        assert!(res_tab_newline.is_err());
        assert_eq!(
            res_tab_newline.unwrap_err().to_string(),
            "Relation type cannot be empty or whitespace-only"
        );

        // Trimmed relation type success
        manager.create_association(&id_a, &id_b, "   related_to \n  ")?;
        let outgoing = manager.get_associations(&id_a, Some("outgoing"))?;
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].relation_type, "related_to");

        // 2. Self-associations
        let res_self = manager.create_association(&id_a, &id_a, "depends_on");
        assert!(res_self.is_err());
        assert!(res_self
            .unwrap_err()
            .to_string()
            .contains("Self-referential associations are not allowed"));

        // 3. Bidirectional direction edge cases
        // Test valid directions
        let outgoing_ok = manager.get_associations(&id_a, Some("outgoing"));
        assert!(outgoing_ok.is_ok());

        let incoming_ok = manager.get_associations(&id_a, Some("incoming"));
        assert!(incoming_ok.is_ok());

        let both_ok = manager.get_associations(&id_a, Some("both"));
        assert!(both_ok.is_ok());

        // Test invalid/case-sensitive direction values
        let res_caps = manager.get_associations(&id_a, Some("OUTGOING"));
        assert!(res_caps.is_err());
        assert!(res_caps
            .unwrap_err()
            .to_string()
            .contains("Invalid direction: 'OUTGOING'"));

        let res_mixed = manager.get_associations(&id_a, Some("Incoming"));
        assert!(res_mixed.is_err());
        assert!(res_mixed
            .unwrap_err()
            .to_string()
            .contains("Invalid direction: 'Incoming'"));

        let res_padded = manager.get_associations(&id_a, Some(" both "));
        assert!(res_padded.is_err());
        assert!(res_padded
            .unwrap_err()
            .to_string()
            .contains("Invalid direction: ' both '"));

        // 4. Missing nodes
        let res_missing_target =
            manager.create_association(&id_a, "missing-target-id", "depends_on");
        assert!(res_missing_target.is_err());
        assert!(res_missing_target
            .unwrap_err()
            .to_string()
            .contains("Target memory with ID 'missing-target-id' not found"));

        let res_missing_source =
            manager.create_association("missing-source-id", &id_b, "depends_on");
        assert!(res_missing_source.is_err());
        assert!(res_missing_source
            .unwrap_err()
            .to_string()
            .contains("Source memory with ID 'missing-source-id' not found"));

        let res_get_missing_source = manager.get_associations("missing-source-id", Some("both"));
        assert!(res_get_missing_source.is_err());
        assert!(res_get_missing_source
            .unwrap_err()
            .to_string()
            .contains("Source memory with ID 'missing-source-id' not found"));

        Ok(())
    }

    #[test]
    fn test_milestone5_exact_state_transition_and_clamp_recovery() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;
        let id = manager.store(
            MemoryLayer::Experience,
            None,
            "Exact state transition testing".to_string(),
            vec![],
        )?;

        // 1. Initial score starts at 1.0
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 1.0);

        // 2. Success multiplies by 1.1 -> 1.1
        manager.evaluate_experience(&id, true)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert!((entry.evaluation_score - 1.1).abs() < 1e-9);

        // 3. Failure multiplies by 0.8 -> 0.88
        manager.evaluate_experience(&id, false)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert!((entry.evaluation_score - 0.88).abs() < 1e-9);

        // 4. Successes clamp at 5.0
        for _ in 0..20 {
            manager.evaluate_experience(&id, true)?;
        }
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 5.0);

        // 5. Success again remains at 5.0
        manager.evaluate_experience(&id, true)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 5.0);

        // 6. Failure immediately drops to 4.0
        manager.evaluate_experience(&id, false)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert!((entry.evaluation_score - 4.0).abs() < 1e-9);

        // 7. Failures clamp at 0.1
        for _ in 0..30 {
            manager.evaluate_experience(&id, false)?;
        }
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 0.1);

        // 8. Failure again remains at 0.1
        manager.evaluate_experience(&id, false)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert_eq!(entry.evaluation_score, 0.1);

        // 9. Success immediately rises to 0.11
        manager.evaluate_experience(&id, true)?;
        let entry = manager.storage.get_memory_by_id(&id)?.unwrap();
        assert!((entry.evaluation_score - 0.11).abs() < 1e-9);

        Ok(())
    }

    #[test]
    fn test_milestone5_non_experience_layer_rejection() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        let id_rule = manager.store(MemoryLayer::Rule, None, "Rule content".to_string(), vec![])?;
        let id_persona = manager.store(
            MemoryLayer::Persona,
            None,
            "Persona content".to_string(),
            vec![],
        )?;
        let id_session = manager.store(
            MemoryLayer::Session,
            Some("session-abc".to_string()),
            "Session content".to_string(),
            vec![],
        )?;

        // Evaluating non-Experience layers returns error
        assert!(manager.evaluate_experience(&id_rule, true).is_err());
        assert!(manager.evaluate_experience(&id_persona, true).is_err());
        assert!(manager.evaluate_experience(&id_session, true).is_err());

        // Their scores remain 1.0
        assert_eq!(
            manager
                .storage
                .get_memory_by_id(&id_rule)?
                .unwrap()
                .evaluation_score,
            1.0
        );
        assert_eq!(
            manager
                .storage
                .get_memory_by_id(&id_persona)?
                .unwrap()
                .evaluation_score,
            1.0
        );
        assert_eq!(
            manager
                .storage
                .get_memory_by_id(&id_session)?
                .unwrap()
                .evaluation_score,
            1.0
        );

        Ok(())
    }

    #[test]
    fn test_milestone5_database_constraint_isolation() -> Result<()> {
        let storage = SqliteStorage::new(":memory:")?;

        // 1. Session layer with session_id = None must fail due to check_session_id
        let entry_invalid_session = MemoryEntry {
            id: "session-fail".to_string(),
            layer: MemoryLayer::Session,
            session_id: None,
            content: "content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![],
            status: MemoryStatus::Active,
        };
        assert!(storage.insert_memory(&entry_invalid_session).is_err());

        // 2. Rule layer with session_id populated must fail due to check_session_id
        let entry_invalid_rule = MemoryEntry {
            id: "rule-fail".to_string(),
            layer: MemoryLayer::Rule,
            session_id: Some("session-xyz".to_string()),
            content: "content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![],
            status: MemoryStatus::Active,
        };
        assert!(storage.insert_memory(&entry_invalid_rule).is_err());

        // 3. Score < 0.0 must fail due to check_evaluation_score CHECK (evaluation_score >= 0.0)
        let entry_neg_score = MemoryEntry {
            id: "neg-score".to_string(),
            layer: MemoryLayer::Experience,
            session_id: None,
            content: "content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: -0.5,
            embedding: vec![],
            status: MemoryStatus::Active,
        };
        assert!(storage.insert_memory(&entry_neg_score).is_err());

        // 4. Invalid layer name insert via raw SQL must fail due to check_layer
        let res_raw_sql = storage.conn.execute(
            "INSERT INTO memories (id, layer, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score)
             VALUES ('invalid-layer-id', 'InvalidLayerName', 'content', '[]', X'', '2026-06-05T00:00:00Z', '2026-06-05T00:00:00Z', 0, 1.0)",
            []
        );
        assert!(res_raw_sql.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_milestone5_concurrency_safety() -> Result<()> {
        let manager = Arc::new(tokio::sync::Mutex::new(MemoryManager::new(":memory:")?));

        // Store an experience memory
        let exp_id = {
            let mut mgr = manager.lock().await;
            mgr.store(
                MemoryLayer::Experience,
                None,
                "Concurrent evolution test".to_string(),
                vec![],
            )?
        };

        // Spawn 20 tasks that concurrently query/evolve the experience memory
        let mut handles = vec![];
        for i in 0..20 {
            let manager_clone = manager.clone();
            let exp_id_clone = exp_id.clone();
            let handle = tokio::spawn(async move {
                let success = i % 2 == 0;
                let mut mgr = manager_clone.lock().await;

                // Do a retrieve
                let _ = mgr.retrieve("evolution test", None).unwrap();

                // Do an evaluation
                mgr.evaluate_experience(&exp_id_clone, success).unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify the final score is successfully updated and within bounds
        let mgr = manager.lock().await;
        let entry = mgr.storage.get_memory_by_id(&exp_id)?.unwrap();
        assert!(entry.evaluation_score >= 0.1 && entry.evaluation_score <= 5.0);

        Ok(())
    }

    #[test]
    fn test_milestone5_search_dominance_and_starvation_resistance() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // 1. Insert Rule memory R (with a small matching content)
        let id_r = manager.store(
            MemoryLayer::Rule,
            None,
            "Dominance Rule matches query".to_string(),
            vec![],
        )?;

        // 2. Insert highly-evolved Experience memory E (score 5.0, matching content)
        let id_e = manager.store(
            MemoryLayer::Experience,
            None,
            "Dominance Experience matches query".to_string(),
            vec![],
        )?;
        // Manually update its evaluation score to 5.0
        let mut entry_e = manager.storage.get_memory_by_id(&id_e)?.unwrap();
        entry_e.evaluation_score = 5.0;
        manager.storage.update_memory(&entry_e)?;

        // 3. Insert Session memory S (highly matching content, but session weight is small = 1.0)
        let id_s = manager.store(
            MemoryLayer::Session,
            Some("session-123".to_string()),
            "Dominance Session highly matches query".to_string(),
            vec![],
        )?;

        // Retrieve with the query "Dominance matches query"
        let results =
            manager.retrieve("Dominance matches query", Some("session-123".to_string()))?;

        // Verify that all three are returned (none is starved out from the top 5)
        let ids: Vec<String> = results.iter().map(|r| r.memory.id.clone()).collect();
        assert!(ids.contains(&id_r), "Rule memory was starved");
        assert!(ids.contains(&id_e), "Experience memory was starved");
        assert!(ids.contains(&id_s), "Session memory was starved");

        Ok(())
    }

    #[test]
    fn test_milestone5_nan_infinity_input_safety() -> Result<()> {
        // 1. Test NaN/Infinity safety inside EvolutionEngine
        let engine =
            crate::evolution::EvolutionEngine::new(crate::evolution::EvolutionConfig::default());

        // If current score is NaN, it should fallback to min_score (0.1)
        let nan_eval = engine.evaluate(std::f64::NAN, true, &MemoryLayer::Experience);
        assert_eq!(nan_eval, 0.1);

        // If current score is Infinity, it should fallback to min_score (0.1)
        let inf_eval = engine.evaluate(std::f64::INFINITY, true, &MemoryLayer::Experience);
        assert_eq!(inf_eval, 0.1);

        // 2. Test sorting robustness when search results have NaN or Infinity
        let mut candidates = vec![
            SearchResult {
                memory: MemoryEntry {
                    id: "nan-mem".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "nan content".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: std::f64::NAN,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
            SearchResult {
                memory: MemoryEntry {
                    id: "inf-mem".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "inf content".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: std::f64::INFINITY,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
            SearchResult {
                memory: MemoryEntry {
                    id: "normal-mem".to_string(),
                    layer: MemoryLayer::Session,
                    session_id: None,
                    content: "normal content".to_string(),
                    tags: vec![],
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 0,
                    evaluation_score: 1.0,
                    embedding: vec![],
                    status: MemoryStatus::Active,
                },
                final_score: 1.2,
                size: 0,
                created_at: String::new(),
                cumulative_size: 0,
            },
        ];

        // Should sort without panicking
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(candidates.len(), 3);

        Ok(())
    }

    #[test]
    fn test_conflict_detection_and_resolution_flow() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // 1. Store a memory
        let content_old = "The project uses Postgres 15 database".to_string();
        let id_old = manager.store(
            MemoryLayer::Experience,
            None,
            content_old.clone(),
            vec!["database".to_string()],
        )?;

        // 2. Query conflict candidates for similar content
        let content_new = "The project database has been upgraded to Postgres 16".to_string();
        let candidates =
            manager.get_conflict_candidates(&content_new, None, Some(0.65), Some(5))?;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].memory.id, id_old);
        assert!(candidates[0].similarity > 0.65);

        // 3. Resolve conflict by deprecating old memory and adding new memory and association
        let new_id = uuid::Uuid::new_v4().to_string();
        let new_mem = crate::models::MemoryEntryInput {
            id: Some(new_id.clone()),
            layer: MemoryLayer::Experience,
            session_id: None,
            content: content_new,
            tags: vec!["database".to_string()],
        };

        let new_assoc = crate::models::AssociationInput {
            source_id: new_id.clone(),
            target_id: id_old.clone(),
            relation_type: "replaces".to_string(),
        };

        let inserted = manager.resolve_conflict(
            &[id_old.clone()], // deprecate
            &[],               // delete
            &[new_mem],        // new memories
            &[new_assoc],      // new associations
        )?;

        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0], new_id);

        // 4. Verify that the deprecated memory status is changed to Deprecated
        let old_entry = manager.storage.get_memory_by_id(&id_old)?.unwrap();
        assert_eq!(old_entry.status, MemoryStatus::Deprecated);

        // 5. Verify new memory is Active
        let new_entry = manager.storage.get_memory_by_id(&new_id)?.unwrap();
        assert_eq!(new_entry.status, MemoryStatus::Active);

        // 6. Verify association is established
        let assocs = manager.storage.get_associations(&new_id, "outgoing")?;
        assert_eq!(assocs.len(), 1);
        assert_eq!(assocs[0].target_id, id_old);
        assert_eq!(assocs[0].relation_type, "replaces");

        Ok(())
    }

    #[test]
    fn test_session_compaction_and_purging_flow() -> Result<()> {
        let mut manager = MemoryManager::new(":memory:")?;

        // 1. Store session memories
        let sid = "session-test".to_string();
        let id_1 = manager.store(
            MemoryLayer::Session,
            Some(sid.clone()),
            "Memory 1".to_string(),
            vec![],
        )?;
        let id_2 = manager.store(
            MemoryLayer::Session,
            Some(sid.clone()),
            "Memory 2".to_string(),
            vec![],
        )?;
        let id_3 = manager.store(
            MemoryLayer::Session,
            Some(sid.clone()),
            "Memory 3".to_string(),
            vec![],
        )?;

        // 2. Retrieve session memories
        let session_mems = manager.get_session_memories(&sid, None)?;
        assert_eq!(session_mems.len(), 3);
        assert_eq!(session_mems[0].id, id_1);
        assert_eq!(session_mems[1].id, id_2);
        assert_eq!(session_mems[2].id, id_3);

        // 3. Purge session, preserving id_2 (soft deprecation)
        let count = manager.purge_session_memories(&sid, &[id_2.clone()], false)?;
        assert_eq!(count, 2); // id_1 and id_3 are soft deprecated

        // Check statuses
        assert_eq!(
            manager.storage.get_memory_by_id(&id_1)?.unwrap().status,
            MemoryStatus::Deprecated
        );
        assert_eq!(
            manager.storage.get_memory_by_id(&id_2)?.unwrap().status,
            MemoryStatus::Active
        );
        assert_eq!(
            manager.storage.get_memory_by_id(&id_3)?.unwrap().status,
            MemoryStatus::Deprecated
        );

        // Active retrieve should only return preserved memory
        let active_mems = manager.get_session_memories(&sid, None)?;
        assert_eq!(active_mems.len(), 1);
        assert_eq!(active_mems[0].id, id_2);

        // 4. Permanently purge the deprecated session memories
        let count_perm = manager.purge_session_memories(&sid, &[id_2.clone()], true)?;
        assert_eq!(count_perm, 2);

        // They must be completely gone
        assert!(manager.storage.get_memory_by_id(&id_1)?.is_none());
        assert!(manager.storage.get_memory_by_id(&id_3)?.is_none());
        assert!(manager.storage.get_memory_by_id(&id_2)?.is_some());

        Ok(())
    }
}
