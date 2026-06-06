use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryLayer {
    #[serde(alias = "Principle")]
    Rule,
    Persona,
    Experience,
    Session,
}

impl MemoryLayer {
    pub fn base_weight(&self) -> f64 {
        match self {
            MemoryLayer::Rule => 10.0,
            MemoryLayer::Persona => 5.0,
            MemoryLayer::Experience => 3.0,
            MemoryLayer::Session => 1.0,
        }
    }

    pub fn decay_rate(&self) -> f64 {
        match self {
            MemoryLayer::Rule => 0.0,
            MemoryLayer::Persona => 0.001,
            MemoryLayer::Experience => 0.05,
            MemoryLayer::Session => 0.2, // Fast decay
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryStatus {
    Active,
    Deprecated,
    PendingResolution,
}

impl std::fmt::Display for MemoryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryStatus::Active => write!(f, "Active"),
            MemoryStatus::Deprecated => write!(f, "Deprecated"),
            MemoryStatus::PendingResolution => write!(f, "PendingResolution"),
        }
    }
}

pub fn default_memory_status() -> MemoryStatus {
    MemoryStatus::Active
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub layer: MemoryLayer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub access_count: u32,
    pub evaluation_score: f64, // Used for Experience adaptation
    #[serde(default)]
    pub embedding: Vec<f32>,
    #[serde(default = "default_memory_status")]
    pub status: MemoryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Association {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memory: MemoryEntry,
    pub final_score: f64,
    pub size: usize,
    pub created_at: String,
    pub cumulative_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictCandidate {
    pub memory: MemoryEntry,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntryInput {
    #[serde(default)]
    pub id: Option<String>,
    pub layer: MemoryLayer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssociationInput {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
}
