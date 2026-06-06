use rusqlite::{Connection, params};
use std::path::Path;
use anyhow::{Result, Context};
use crate::models::{MemoryEntry, MemoryLayer, Association};
use chrono::{DateTime, Utc};

pub struct SqliteStorage {
    pub conn: Connection,
}

impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        
        // Enable WAL mode & foreign keys
        conn.pragma_update(None, "journal_mode", &"WAL")?;
        conn.pragma_update(None, "foreign_keys", &"ON")?;


        let storage = SqliteStorage { conn };
        storage.initialize_tables()?;
        
        Ok(storage)
    }

    fn initialize_tables(&self) -> Result<()> {
        // Create unified memories table
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                layer TEXT NOT NULL,
                session_id TEXT,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL,
                last_accessed TEXT NOT NULL,
                access_count INTEGER NOT NULL DEFAULT 0,
                evaluation_score REAL NOT NULL DEFAULT 1.0,
                CONSTRAINT check_layer CHECK (layer IN ('Rule', 'Persona', 'Experience', 'Session')),
                CONSTRAINT check_session_id CHECK (
                    (layer = 'Session' AND session_id IS NOT NULL AND length(session_id) > 0) OR
                    (layer != 'Session' AND session_id IS NULL)
                ),
                CONSTRAINT check_evaluation_score CHECK (evaluation_score >= 0.0)
            );",
            [],
        ).context("Failed to create memories table")?;

        // Create indexes on memories table
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_layer_session ON memories (layer, session_id);",
            [],
        ).context("Failed to create idx_memories_layer_session")?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories (created_at);",
            [],
        ).context("Failed to create idx_memories_created_at")?;

        // Create graph_associations with strict SQLite foreign keys targeting memories(id) with ON DELETE CASCADE
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS graph_associations (
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (source_id, target_id, relation_type),
                FOREIGN KEY (source_id) REFERENCES memories(id) ON DELETE CASCADE,
                FOREIGN KEY (target_id) REFERENCES memories(id) ON DELETE CASCADE
            );",
            [],
        ).context("Failed to create graph_associations table")?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_graph_associations_target ON graph_associations(target_id);",
            [],
        ).context("Failed to create idx_graph_associations_target")?;

        Ok(())
    }

    fn row_to_memory(&self, row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
        let id: String = row.get(0)?;
        let layer_str: String = row.get(1)?;
        let session_id: Option<String> = row.get(2)?;
        let content: String = row.get(3)?;
        let tags_str: String = row.get(4)?;
        let embedding_bytes: Vec<u8> = row.get(5)?;
        let created_at_str: String = row.get(6)?;
        let last_accessed_str: String = row.get(7)?;
        let access_count: u32 = row.get(8)?;
        let evaluation_score: f64 = row.get(9)?;

        let layer = match layer_str.as_str() {
            "Rule" | "Principle" => MemoryLayer::Rule,
            "Persona" => MemoryLayer::Persona,
            "Experience" => MemoryLayer::Experience,
            "Session" => MemoryLayer::Session,
            _ => return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("Unknown layer: {}", layer_str))),
            )),
        };

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?;
        
        let last_accessed = DateTime::parse_from_rfc3339(&last_accessed_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?;

        let embedding = bytes_to_f32_vec(&embedding_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Blob, Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))))?;

        Ok(MemoryEntry {
            id,
            layer,
            session_id,
            content,
            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
            created_at,
            last_accessed,
            access_count,
            evaluation_score,
            embedding,
        })
    }

    pub fn insert_memory(&self, entry: &MemoryEntry) -> Result<()> {
        let layer_str = match entry.layer {
            MemoryLayer::Rule => "Rule",
            MemoryLayer::Persona => "Persona",
            MemoryLayer::Experience => "Experience",
            MemoryLayer::Session => "Session",
        };
        let tags_str = serde_json::to_string(&entry.tags)?;
        let embedding_bytes = f32_vec_to_bytes(&entry.embedding);
        let created_at_str = entry.created_at.to_rfc3339();
        let last_accessed_str = entry.last_accessed.to_rfc3339();

        self.conn.execute(
            "INSERT INTO memories (id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.id,
                layer_str,
                entry.session_id,
                entry.content,
                tags_str,
                embedding_bytes,
                created_at_str,
                last_accessed_str,
                entry.access_count,
                entry.evaluation_score
            ],
        )?;
        Ok(())
    }

    pub fn update_memory(&self, entry: &MemoryEntry) -> Result<()> {
        let layer_str = match entry.layer {
            MemoryLayer::Rule => "Rule",
            MemoryLayer::Persona => "Persona",
            MemoryLayer::Experience => "Experience",
            MemoryLayer::Session => "Session",
        };
        let tags_str = serde_json::to_string(&entry.tags)?;
        let embedding_bytes = f32_vec_to_bytes(&entry.embedding);
        let created_at_str = entry.created_at.to_rfc3339();
        let last_accessed_str = entry.last_accessed.to_rfc3339();

        self.conn.execute(
            "UPDATE memories SET layer = ?2, session_id = ?3, content = ?4, tags = ?5, embedding = ?6, last_accessed = ?7, access_count = ?8, evaluation_score = ?9, created_at = ?10 WHERE id = ?1",
            params![
                entry.id,
                layer_str,
                entry.session_id,
                entry.content,
                tags_str,
                embedding_bytes,
                last_accessed_str,
                entry.access_count,
                entry.evaluation_score,
                created_at_str
            ],
        )?;
        Ok(())
    }

    pub fn update_memories_batch(&self, entries: &[&MemoryEntry]) -> Result<()> {
        self.conn.execute("BEGIN TRANSACTION;", [])?;
        for entry in entries {
            if let Err(e) = self.update_memory(entry) {
                let _ = self.conn.execute("ROLLBACK;", []);
                return Err(e);
            }
        }
        self.conn.execute("COMMIT;", [])?;
        Ok(())
    }

    pub fn get_memory_by_id(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score FROM memories WHERE id = ?1"
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(self.row_to_memory(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn load_all_memories(&self) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score FROM memories"
        )?;
        let rows = stmt.query_map([], |row| self.row_to_memory(row))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn get_relevant_memories(&self, session_id: Option<&str>) -> Result<Vec<MemoryEntry>> {
        let mut stmt = if let Some(_) = session_id {
            self.conn.prepare(
                "SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score 
                 FROM memories 
                 WHERE layer IN ('Rule', 'Persona', 'Experience')
                 UNION ALL
                 SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score 
                 FROM memories 
                 WHERE layer = 'Session' AND session_id = ?1"
            )?
        } else {
            self.conn.prepare(
                "SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score 
                 FROM memories 
                 WHERE layer IN ('Rule', 'Persona', 'Experience')"
            )?
        };

        let mut results = Vec::new();
        if let Some(sid) = session_id {
            let rows = stmt.query_map(params![sid], |row| self.row_to_memory(row))?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let rows = stmt.query_map([], |row| self.row_to_memory(row))?;
            for row in rows {
                results.push(row?);
            }
        }
        Ok(results)

    }

    pub fn create_association(&self, assoc: &Association) -> Result<()> {
        self.conn.execute(
            "INSERT INTO graph_associations (source_id, target_id, relation_type, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source_id, target_id, relation_type) DO UPDATE SET created_at = excluded.created_at",
            params![
                assoc.source_id,
                assoc.target_id,
                assoc.relation_type,
                assoc.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_associations(&self, source_id: &str, direction: &str) -> Result<Vec<Association>> {
        let mut stmt = match direction {
            "incoming" => self.conn.prepare(
                "SELECT source_id, target_id, relation_type, created_at FROM graph_associations WHERE target_id = ?1"
            )?,
            "both" => self.conn.prepare(
                "SELECT source_id, target_id, relation_type, created_at FROM graph_associations WHERE source_id = ?1 OR target_id = ?1"
            )?,
            _ => self.conn.prepare(
                "SELECT source_id, target_id, relation_type, created_at FROM graph_associations WHERE source_id = ?1"
            )?,
        };
        let rows = stmt.query_map(params![source_id], |row| {
            let created_at_str: String = row.get(3)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                ))?;
            Ok(Association {
                source_id: row.get(0)?,
                target_id: row.get(1)?,
                relation_type: row.get(2)?,
                created_at,
            })
        })?;
        
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

pub fn f32_vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &val in v {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

pub fn bytes_to_f32_vec(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() % 4 != 0 {
        return Err("Invalid byte length for Vec<f32>".into());
    }
    let mut vec = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let array: [u8; 4] = chunk.try_into().map_err(|e| format!("{:?}", e))?;
        vec.push(f32::from_le_bytes(array));
    }
    Ok(vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MemoryLayer;

    #[test]
    fn test_in_memory_db_and_insert_retrieve() -> Result<()> {
        let storage = SqliteStorage::new(":memory:")?;

        // 1. Test insert and retrieval of rule
        let entry1 = MemoryEntry {
            id: "rule-1".to_string(),
            layer: MemoryLayer::Rule,
            session_id: None,
            content: "Always reply politely.".to_string(),
            tags: vec!["politeness".to_string(), "rules".to_string()],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![0.1, 0.2, 0.3],
        };
        storage.insert_memory(&entry1)?;

        let retrieved1 = storage.get_memory_by_id("rule-1")?.unwrap();
        assert_eq!(retrieved1.id, "rule-1");
        assert_eq!(retrieved1.layer, MemoryLayer::Rule);
        assert_eq!(retrieved1.content, "Always reply politely.");
        assert_eq!(retrieved1.tags, vec!["politeness".to_string(), "rules".to_string()]);
        assert_eq!(retrieved1.embedding, vec![0.1, 0.2, 0.3]);

        // 2. Test insert and retrieval of session memory
        let entry2 = MemoryEntry {
            id: "session-1".to_string(),
            layer: MemoryLayer::Session,
            session_id: Some("session-abc".to_string()),
            content: "User said they like rust.".to_string(),
            tags: vec!["rust".to_string()],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 1,
            evaluation_score: 1.0,
            embedding: vec![],
        };
        storage.insert_memory(&entry2)?;

        let retrieved2 = storage.get_memory_by_id("session-1")?.unwrap();
        assert_eq!(retrieved2.id, "session-1");
        assert_eq!(retrieved2.session_id, Some("session-abc".to_string()));
        assert_eq!(retrieved2.content, "User said they like rust.");
        assert_eq!(retrieved2.embedding.len(), 0);

        // 3. Test load all memories
        let all = storage.load_all_memories()?;
        assert_eq!(all.len(), 2);

        // 4. Test update memory
        let mut update_entry = retrieved1;
        update_entry.access_count = 5;
        update_entry.evaluation_score = 1.5;
        update_entry.content = "Always reply super politely.".to_string();
        storage.update_memory(&update_entry)?;

        let retrieved_update = storage.get_memory_by_id("rule-1")?.unwrap();
        assert_eq!(retrieved_update.access_count, 5);
        assert_eq!(retrieved_update.content, "Always reply super politely.");
        // Rules evaluation_score is now stored and retrieved correctly
        assert_eq!(retrieved_update.evaluation_score, 1.5);

        // 5. Test experiences evaluation_score persistence
        let entry3 = MemoryEntry {
            id: "experience-1".to_string(),
            layer: MemoryLayer::Experience,
            session_id: None,
            content: "Solved bug by using memory DB.".to_string(),
            tags: vec!["debug".to_string()],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 2.5,
            embedding: vec![],
        };
        storage.insert_memory(&entry3)?;

        let retrieved_exp = storage.get_memory_by_id("experience-1")?.unwrap();
        assert_eq!(retrieved_exp.evaluation_score, 2.5);

        // Update experience evaluation_score
        let mut update_exp = retrieved_exp;
        update_exp.evaluation_score = 3.0;
        storage.update_memory(&update_exp)?;
        let retrieved_exp2 = storage.get_memory_by_id("experience-1")?.unwrap();
        assert_eq!(retrieved_exp2.evaluation_score, 3.0);

        Ok(())
    }

    #[test]
    fn test_graph_associations() -> Result<()> {
        let storage = SqliteStorage::new(":memory:")?;

        // Insert memories first to satisfy foreign key constraints
        let mem_a = MemoryEntry {
            id: "node-a".to_string(),
            layer: MemoryLayer::Rule,
            session_id: None,
            content: "Node A content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![],
        };
        let mem_b = MemoryEntry {
            id: "node-b".to_string(),
            layer: MemoryLayer::Rule,
            session_id: None,
            content: "Node B content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![],
        };
        let mem_c = MemoryEntry {
            id: "node-c".to_string(),
            layer: MemoryLayer::Rule,
            session_id: None,
            content: "Node C content".to_string(),
            tags: vec![],
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 0,
            evaluation_score: 1.0,
            embedding: vec![],
        };
        storage.insert_memory(&mem_a)?;
        storage.insert_memory(&mem_b)?;
        storage.insert_memory(&mem_c)?;

        let assoc1 = Association {
            source_id: "node-a".to_string(),
            target_id: "node-b".to_string(),
            relation_type: "related_to".to_string(),
            created_at: Utc::now(),
        };
        storage.create_association(&assoc1)?;

        let assoc2 = Association {
            source_id: "node-a".to_string(),
            target_id: "node-c".to_string(),
            relation_type: "depends_on".to_string(),
            created_at: Utc::now(),
        };
        storage.create_association(&assoc2)?;

        let assocs = storage.get_associations("node-a", "outgoing")?;
        assert_eq!(assocs.len(), 2);
        assert!(assocs.iter().any(|a| a.target_id == "node-b" && a.relation_type == "related_to"));
        assert!(assocs.iter().any(|a| a.target_id == "node-c" && a.relation_type == "depends_on"));

        // Verify ON DELETE CASCADE
        storage.conn.execute("DELETE FROM memories WHERE id = 'node-b'", [])?;
        let assocs = storage.get_associations("node-a", "outgoing")?;
        assert_eq!(assocs.len(), 1);
        assert_eq!(assocs[0].target_id, "node-c");

        Ok(())
    }

    #[test]
    fn test_query_plan_and_performance() -> Result<()> {
        let storage = SqliteStorage::new(":memory:")?;

        // Insert 100 Rule, 100 Persona, 100 Experience, and 1000 Session memories (across 10 different sessions)
        for i in 0..100 {
            storage.insert_memory(&MemoryEntry {
                id: format!("rule-{}", i),
                layer: MemoryLayer::Rule,
                session_id: None,
                content: format!("Rule content {}", i),
                tags: vec![],
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 0,
                evaluation_score: 1.0,
                embedding: vec![],
            })?;
            storage.insert_memory(&MemoryEntry {
                id: format!("persona-{}", i),
                layer: MemoryLayer::Persona,
                session_id: None,
                content: format!("Persona content {}", i),
                tags: vec![],
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 0,
                evaluation_score: 1.0,
                embedding: vec![],
            })?;
            storage.insert_memory(&MemoryEntry {
                id: format!("experience-{}", i),
                layer: MemoryLayer::Experience,
                session_id: None,
                content: format!("Experience content {}", i),
                tags: vec![],
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 0,
                evaluation_score: 1.0,
                embedding: vec![],
            })?;
        }

        for i in 0..1000 {
            let s_id = format!("session-{}", i % 10);
            storage.insert_memory(&MemoryEntry {
                id: format!("session-mem-{}", i),
                layer: MemoryLayer::Session,
                session_id: Some(s_id),
                content: format!("Session memory content {}", i),
                tags: vec![],
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 0,
                evaluation_score: 1.0,
                embedding: vec![],
            })?;
        }

        // 1. Verify Query Plan for Session Query (with session_id)
        let query_with_session = 
            "SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score 
             FROM memories 
             WHERE layer IN ('Rule', 'Persona', 'Experience')
             UNION ALL
             SELECT id, layer, session_id, content, tags, embedding, created_at, last_accessed, access_count, evaluation_score 
             FROM memories 
             WHERE layer = 'Session' AND session_id = ?1";

        let mut stmt = storage.conn.prepare(&format!("EXPLAIN QUERY PLAN {}", query_with_session))?;
        let mut rows = stmt.query(params!["session-0"])?;
        let mut plan_details = Vec::new();
        while let Some(row) = rows.next()? {
            let detail: String = row.get(3)?;
            plan_details.push(detail);
        }

        // Output plan details for debugging/logging
        println!("Query Plan for get_relevant_memories(Some(session_id)):");
        for detail in &plan_details {
            println!("  {}", detail);
        }

        // Assert that SQLite uses idx_memories_layer_session and avoids SCAN TABLE memories
        let uses_index = plan_details.iter().any(|detail| {
            detail.contains("USING INDEX idx_memories_layer_session") || detail.contains("USING COVERING INDEX idx_memories_layer_session")
        });
        let has_full_scan = plan_details.iter().any(|detail| {
            detail.contains("SCAN TABLE memories") && !detail.contains("USING INDEX")
        });

        assert!(uses_index, "Query plan should use idx_memories_layer_session index");
        assert!(!has_full_scan, "Query plan should avoid full table scan of memories table");

        // 2. Verify Session Isolation
        let retrieved = storage.get_relevant_memories(Some("session-0"))?;
        // Should contain 300 global memories (100 rules + 100 personas + 100 experiences)
        // plus 100 session-0 memories (out of 1000 session memories, modulo 10)
        assert_eq!(retrieved.len(), 400);
        for mem in &retrieved {
            if mem.layer == MemoryLayer::Session {
                assert_eq!(mem.session_id.as_deref(), Some("session-0"), "Session isolation failed: got session ID {:?}", mem.session_id);
            }
        }

        // Query with None (should get only global memories)
        let retrieved_none = storage.get_relevant_memories(None)?;
        assert_eq!(retrieved_none.len(), 300);
        for mem in &retrieved_none {
            assert_ne!(mem.layer, MemoryLayer::Session, "Session memory returned when session_id was None");
        }

        // 3. Performance Timing
        let start_time = std::time::Instant::now();
        let iterations = 100;
        for i in 0..iterations {
            let s_id = format!("session-{}", i % 10);
            let _ = storage.get_relevant_memories(Some(&s_id))?;
        }
        let elapsed = start_time.elapsed();
        println!("Performance check: {} queries took {:?}", iterations, elapsed);

        Ok(())
    }
}
