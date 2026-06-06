use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl McpResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        McpResponse { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }
    pub fn error(id: Option<serde_json::Value>, error: JsonRpcError) -> Self {
        McpResponse { jsonrpc: "2.0".into(), id, result: None, error: Some(error) }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcError {
    pub fn method_not_found() -> Self { JsonRpcError { code: -32601, message: "Method not found".into() } }
    pub fn invalid_params(msg: &str) -> Self { JsonRpcError { code: -32602, message: msg.into() } }
    pub fn internal(msg: String) -> Self { JsonRpcError { code: -32603, message: msg } }
}

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Association {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub memory: MemoryEntry,
    pub final_score: f64,
}
