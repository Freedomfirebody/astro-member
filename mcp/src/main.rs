use anyhow::Result;
use astro_member::memory_manager::MemoryManager;
use astro_member::models;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

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
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn error(id: Option<serde_json::Value>, error: JsonRpcError) -> Self {
        McpResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcError {
    pub fn method_not_found() -> Self {
        JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
        }
    }
    pub fn invalid_params(msg: &str) -> Self {
        JsonRpcError {
            code: -32602,
            message: msg.into(),
        }
    }
    pub fn internal(msg: String) -> Self {
        JsonRpcError {
            code: -32603,
            message: msg,
        }
    }
}

fn parse_and_validate_request(line: &str) -> Result<McpRequest, serde_json::Value> {
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Err(json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {}", e)
                },
                "id": null
            }));
        }
    };

    let is_valid = value.is_object()
        && value.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0")
        && value.get("method").and_then(|v| v.as_str()).is_some();

    if !is_valid {
        return Err(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32600,
                "message": "Invalid Request"
            },
            "id": value.get("id").cloned().unwrap_or(serde_json::Value::Null)
        }));
    }

    match serde_json::from_value::<McpRequest>(value) {
        Ok(req) => Ok(req),
        Err(e) => Err(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32600,
                "message": format!("Invalid Request: {}", e)
            },
            "id": null
        })),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--init" || args[1] == "init") {
        println!("Initializing Astro-Member storage and pre-downloading embedding models...");
        let manager = MemoryManager::new(".mcp_memory_storage")?;
        match manager.embedding_manager.generate_passage_embedding("init") {
            Ok(_) => {
                println!("Success: Database and embedding models initialized successfully!");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: Failed to initialize embedding models: {:?}", e);
                std::process::exit(1);
            }
        }
    }

    // Initialize the root storage directory
    let manager = Arc::new(Mutex::new(MemoryManager::new(".mcp_memory_storage")?));

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    // Standard MCP loop listening on stdin
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        match parse_and_validate_request(&line) {
            Ok(request) => {
                let has_id = request.id.is_some();
                let response = handle_request(request, manager.clone()).await;
                if has_id {
                    let serialized = serde_json::to_string(&response)?;
                    writeln!(stdout, "{}", serialized)?;
                    stdout.flush()?;
                }
            }
            Err(err_resp) => {
                writeln!(stdout, "{}", err_resp)?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}

async fn handle_request(req: McpRequest, manager: Arc<Mutex<MemoryManager>>) -> McpResponse {
    let mut mgr = manager.lock().await;

    let result = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "astro-member",
                "version": "0.2.0"
            }
        })),
        "notifications/initialized" | "initialized" => Ok(json!({})),
        "mcp.server.info" => Ok(json!({
            "name": "astro-member",
            "version": "0.2.0",
            "capabilities": {
                "tools": true
            }
        })),
        "tools/list" | "mcp.tools.list" => Ok(json!({
            "tools": [
                {
                    "name": "store_memory",
                    "description": "Store a memory conceptually into different hierarchical layers.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "layer": { "type": "string", "enum": ["Rule", "Persona", "Experience", "Session"] },
                            "session_id": { "type": "string", "description": "Required if layer is Session" },
                            "content": { "type": "string" },
                            "context_tags": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["layer", "content"]
                    }
                },
                {
                    "name": "retrieve_memory",
                    "description": "Retrieve memories across layers using cohesive weighted search.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "session_id": { "type": "string" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "evaluate_experience",
                    "description": "Evaluate an experience to determine if it achieved its goal (modifies its weight).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "memory_id": { "type": "string" },
                            "success": { "type": "boolean" }
                        },
                        "required": ["memory_id", "success"]
                    }
                },
                {
                    "name": "get_memory_by_id",
                    "description": "Retrieve a specific memory by its unique ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "The unique ID of the memory to retrieve." },
                            "session_id": { "type": "string", "description": "The session ID, required if the target memory is in the Session layer." }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "create_association",
                    "description": "Create a semantic association/relation between two existing memories in the graph database.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "source_id": { "type": "string", "description": "The ID of the source memory." },
                            "target_id": { "type": "string", "description": "The ID of the target memory." },
                            "relation_type": { "type": "string", "description": "The type of association (e.g., related_to, depends_on)." }
                        },
                        "required": ["source_id", "target_id", "relation_type"]
                    }
                },
                {
                    "name": "get_associations",
                    "description": "Retrieve semantic associations/relations originating from or targeting a memory.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "source_id": { "type": "string", "description": "The ID of the source memory to query relations for." },
                            "direction": {
                                "type": "string",
                                "enum": ["outgoing", "incoming", "both"],
                                "description": "The direction of associations to retrieve relative to the source memory. Default is outgoing."
                            }
                        },
                        "required": ["source_id"]
                    }
                },
                {
                    "name": "get_conflict_candidates",
                    "description": "Find active memories that are semantically similar to the proposed content.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string", "description": "The proposed new memory content." },
                            "session_id": { "type": "string", "description": "Scope search to this session (optional)." },
                            "threshold": { "type": "number", "description": "Minimum similarity score to qualify as a conflict. Defaults to 0.70." },
                            "limit": { "type": "integer", "description": "Max candidates to return. Defaults to 5." }
                        },
                        "required": ["content"]
                    }
                },
                {
                    "name": "resolve_conflict",
                    "description": "Atomically execute a set of deprecations, deletions, memory insertions, and association updates to resolve conflicts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "deprecate_ids": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "IDs of existing memories to soft-deprecate."
                            },
                            "delete_ids": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "IDs of existing memories to permanently delete."
                            },
                            "new_memories": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string", "description": "Optional custom UUID." },
                                        "layer": { "type": "string", "enum": ["Rule", "Persona", "Experience", "Session"] },
                                        "session_id": { "type": "string", "description": "Required if layer is Session." },
                                        "content": { "type": "string" },
                                        "tags": { "type": "array", "items": { "type": "string" } }
                                    },
                                    "required": ["layer", "content"]
                                },
                                "description": "New memory entries to insert."
                            },
                            "new_associations": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "source_id": { "type": "string" },
                                        "target_id": { "type": "string" },
                                        "relation_type": { "type": "string" }
                                    },
                                    "required": ["source_id", "target_id", "relation_type"]
                                },
                                "description": "Graph associations to write."
                            }
                        }
                    }
                },
                {
                    "name": "get_session_memories",
                    "description": "Retrieve all active memories associated with a given session in chronological order.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "The target session ID." },
                            "limit": { "type": "integer", "description": "Max memories to retrieve (optional)." }
                        },
                        "required": ["session_id"]
                    }
                },
                {
                    "name": "purge_session_memories",
                    "description": "Bulk soft-deprecate or hard-delete memories in a session, preserving specified items.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": { "type": "string", "description": "The session ID to purge." },
                            "preserve_ids": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "IDs to protect from being purged."
                            },
                            "permanent": {
                                "type": "boolean",
                                "description": "If true, permanently delete. If false, soft-deprecate. Defaults to false."
                            }
                        },
                        "required": ["session_id"]
                    }
                }
            ]
        })),
        "tools/call" | "mcp.tools.call" => {
            if let Some(params) = req.params {
                let name = params["name"].as_str().unwrap_or("");
                let args = &params["arguments"];

                match name {
                    "store_memory" => {
                        let layer_str = args["layer"].as_str().unwrap_or("Session");
                        let layer = match layer_str {
                            "Rule" | "Principle" => models::MemoryLayer::Rule,
                            "Persona" => models::MemoryLayer::Persona,
                            "Experience" => models::MemoryLayer::Experience,
                            _ => models::MemoryLayer::Session,
                        };
                        let session_id = args["session_id"].as_str().map(String::from);
                        let content = args["content"].as_str().unwrap_or("").to_string();
                        let tags: Vec<String> = args["context_tags"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        match mgr.store(layer, session_id, content, tags) {
                            Ok(id) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "status": "success", "memory_id": id })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "retrieve_memory" => {
                        let query = args["query"].as_str().unwrap_or("");
                        let session_id = args["session_id"].as_str().map(String::from);

                        match mgr.retrieve(query, session_id) {
                            Ok(results) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "results": results })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "get_memory_by_id" => {
                        let id = args["id"].as_str().unwrap_or("");
                        let session_id = args["session_id"].as_str();
                        if id.is_empty() {
                            Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Parameter 'id' is required and cannot be empty."
                                    }
                                ]
                            }))
                        } else {
                            match mgr.get_memory_by_id(id, session_id) {
                                Ok(Some(mem)) => Ok(json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": serde_json::to_string(&json!({ "status": "success", "memory": mem })).unwrap()
                                        }
                                    ]
                                })),
                                Ok(None) => Ok(json!({
                                    "isError": true,
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": format!("Memory with ID '{}' not found", id)
                                        }
                                    ]
                                })),
                                Err(e) => Ok(json!({
                                    "isError": true,
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": e.to_string()
                                        }
                                    ]
                                })),
                            }
                        }
                    }
                    "evaluate_experience" => {
                        let memory_id = args["memory_id"].as_str().unwrap_or("");
                        let success = args["success"].as_bool().unwrap_or(false);

                        match mgr.evaluate_experience(memory_id, success) {
                            Ok(_) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "status": "evaluated" })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "create_association" => {
                        let source_id = args["source_id"].as_str().unwrap_or("");
                        let target_id = args["target_id"].as_str().unwrap_or("");
                        let relation_type = args["relation_type"].as_str().unwrap_or("");

                        match mgr.create_association(source_id, target_id, relation_type) {
                            Ok(_) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "status": "association_created" })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "get_associations" => {
                        let source_id = args["source_id"].as_str().unwrap_or("");
                        let direction = args["direction"].as_str();

                        match mgr.get_associations(source_id, direction) {
                            Ok(assocs) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "associations": assocs })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "get_conflict_candidates" => {
                        let content = args["content"].as_str().unwrap_or("");
                        let session_id = args["session_id"].as_str().map(String::from);
                        let threshold = args["threshold"].as_f64();
                        let limit = args["limit"].as_u64().map(|l| l as usize);

                        if content.trim().is_empty() {
                            Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Parameter 'content' is required and cannot be empty."
                                    }
                                ]
                            }))
                        } else {
                            match mgr.get_conflict_candidates(content, session_id, threshold, limit)
                            {
                                Ok(candidates) => Ok(json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": serde_json::to_string(&json!({ "candidates": candidates })).unwrap()
                                        }
                                    ]
                                })),
                                Err(e) => Ok(json!({
                                    "isError": true,
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": e.to_string()
                                        }
                                    ]
                                })),
                            }
                        }
                    }
                    "resolve_conflict" => {
                        let deprecate_ids: Vec<String> = args["deprecate_ids"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let delete_ids: Vec<String> = args["delete_ids"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let new_memories: Vec<models::MemoryEntryInput> = args["new_memories"]
                            .as_array()
                            .map(|a| {
                                serde_json::from_value(serde_json::Value::Array(a.clone()))
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();

                        let new_associations: Vec<models::AssociationInput> = args
                            ["new_associations"]
                            .as_array()
                            .map(|a| {
                                serde_json::from_value(serde_json::Value::Array(a.clone()))
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();

                        match mgr.resolve_conflict(
                            &deprecate_ids,
                            &delete_ids,
                            &new_memories,
                            &new_associations,
                        ) {
                            Ok(inserted_ids) => Ok(json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&json!({ "status": "success", "inserted_ids": inserted_ids })).unwrap()
                                    }
                                ]
                            })),
                            Err(e) => Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": e.to_string()
                                    }
                                ]
                            })),
                        }
                    }
                    "get_session_memories" => {
                        let session_id = args["session_id"].as_str().unwrap_or("");
                        let limit = args["limit"].as_u64().map(|l| l as usize);

                        if session_id.trim().is_empty() {
                            Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Parameter 'session_id' is required and cannot be empty."
                                    }
                                ]
                            }))
                        } else {
                            match mgr.get_session_memories(session_id, limit) {
                                Ok(memories) => Ok(json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": serde_json::to_string(&json!({ "memories": memories })).unwrap()
                                        }
                                    ]
                                })),
                                Err(e) => Ok(json!({
                                    "isError": true,
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": e.to_string()
                                        }
                                    ]
                                })),
                            }
                        }
                    }
                    "purge_session_memories" => {
                        let session_id = args["session_id"].as_str().unwrap_or("");
                        let preserve_ids: Vec<String> = args["preserve_ids"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let permanent = args["permanent"].as_bool().unwrap_or(false);

                        if session_id.trim().is_empty() {
                            Ok(json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Parameter 'session_id' is required and cannot be empty."
                                    }
                                ]
                            }))
                        } else {
                            match mgr.purge_session_memories(session_id, &preserve_ids, permanent) {
                                Ok(count) => Ok(json!({
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": serde_json::to_string(&json!({ "status": "success", "purged_count": count })).unwrap()
                                        }
                                    ]
                                })),
                                Err(e) => Ok(json!({
                                    "isError": true,
                                    "content": [
                                        {
                                            "type": "text",
                                            "text": e.to_string()
                                        }
                                    ]
                                })),
                            }
                        }
                    }
                    _ => Ok(json!({
                        "isError": true,
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Tool '{}' not found", name)
                            }
                        ]
                    })),
                }
            } else {
                Err(JsonRpcError::invalid_params("Missing parameters"))
            }
        }
        _ => Err(JsonRpcError::method_not_found()),
    };

    match result {
        Ok(res) => McpResponse::success(req.id, res),
        Err(err) => McpResponse::error(req.id, err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_e2e_jsonrpc_initialize() -> Result<()> {
        let manager = Arc::new(Mutex::new(MemoryManager::new(":memory:")?));

        let req_init = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let resp_init = handle_request(req_init, manager.clone()).await;
        assert!(resp_init.error.is_none());
        let result = resp_init.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "astro-member");

        Ok(())
    }

    #[tokio::test]
    async fn test_e2e_jsonrpc_store_and_retrieve() -> Result<()> {
        let manager = Arc::new(Mutex::new(MemoryManager::new(":memory:")?));

        // 1. Test mcp.server.info
        let req_info = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "mcp.server.info".to_string(),
            params: None,
        };
        let resp_info = handle_request(req_info, manager.clone()).await;
        assert!(resp_info.error.is_none());
        let result = resp_info.result.unwrap();
        assert_eq!(result["name"], "astro-member");

        // 2. Test tools/list
        let req_list = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp_list = handle_request(req_list, manager.clone()).await;
        assert!(resp_list.error.is_none());
        let result = resp_list.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 10);
        assert!(tools.iter().any(|t| t["name"] == "store_memory"));
        assert!(tools.iter().any(|t| t["name"] == "create_association"));
        assert!(tools.iter().any(|t| t["name"] == "get_memory_by_id"));
        for tool in tools {
            assert!(tool.get("parameters").is_none());
        }

        // 3. Test tools/call - store_memory (Rule)
        let req_store = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_memory",
                "arguments": {
                    "layer": "Rule",
                    "content": "Follow standard guidelines.",
                    "context_tags": ["rules"]
                }
            })),
        };
        let resp_store = handle_request(req_store, manager.clone()).await;
        assert!(resp_store.error.is_none());
        let result_store = resp_store.result.unwrap();
        let content_text = result_store["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        assert_eq!(val["status"], "success");
        let memory_id = val["memory_id"].as_str().unwrap().to_string();

        // 4. Test tools/call - retrieve_memory
        let req_retrieve = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(4)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "retrieve_memory",
                "arguments": {
                    "query": "standard guidelines"
                }
            })),
        };
        let resp_retrieve = handle_request(req_retrieve, manager.clone()).await;
        assert!(resp_retrieve.error.is_none());
        let result_retrieve = resp_retrieve.result.unwrap();
        let content_text = result_retrieve["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        let results = val["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["memory"]["id"], memory_id);

        // 5. Test tools/call - evaluate_experience (with fake id, but let's store one first)
        let req_store_exp = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(5)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_memory",
                "arguments": {
                    "layer": "Experience",
                    "content": "Had a success with test writing.",
                    "context_tags": ["testing"]
                }
            })),
        };
        let resp_store_exp = handle_request(req_store_exp, manager.clone()).await;
        let content_text = resp_store_exp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let val: serde_json::Value = serde_json::from_str(&content_text).unwrap();
        let exp_id = val["memory_id"].as_str().unwrap().to_string();

        let req_eval = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(6)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "evaluate_experience",
                "arguments": {
                    "memory_id": exp_id,
                    "success": true
                }
            })),
        };
        let resp_eval = handle_request(req_eval, manager.clone()).await;
        assert!(resp_eval.error.is_none());
        let content_text = resp_eval.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let val: serde_json::Value = serde_json::from_str(&content_text).unwrap();
        assert_eq!(val["status"], "evaluated");

        // Verify the score updated
        let manager_lock = manager.lock().await;
        let exp_entry = manager_lock
            .storage
            .get_memory_by_id(&exp_id)
            .unwrap()
            .unwrap();
        assert!((exp_entry.evaluation_score - 1.1).abs() < 1e-9);
        drop(manager_lock);

        // 6. Test tools/call - create_association
        let req_assoc = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(11)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "create_association",
                "arguments": {
                    "source_id": memory_id,
                    "target_id": exp_id,
                    "relation_type": "relates_to"
                }
            })),
        };
        let resp_assoc = handle_request(req_assoc, manager.clone()).await;
        assert!(resp_assoc.error.is_none());
        let result_assoc = resp_assoc.result.unwrap();
        let content_text = result_assoc["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        assert_eq!(val["status"], "association_created");

        // 7. Test tools/call - get_associations
        let req_get_assoc = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(12)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_associations",
                "arguments": {
                    "source_id": memory_id
                }
            })),
        };
        let resp_get_assoc = handle_request(req_get_assoc, manager.clone()).await;
        assert!(resp_get_assoc.error.is_none());
        let result_get_assoc = resp_get_assoc.result.unwrap();
        let content_text = result_get_assoc["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        let assocs = val["associations"].as_array().unwrap();
        assert_eq!(assocs.len(), 1);
        assert_eq!(assocs[0]["source_id"], memory_id);
        assert_eq!(assocs[0]["target_id"], exp_id);
        assert_eq!(assocs[0]["relation_type"], "relates_to");

        // 7b. Test tools/call - get_memory_by_id
        let req_get_mem = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(15)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_memory_by_id",
                "arguments": {
                    "id": memory_id
                }
            })),
        };
        let resp_get_mem = handle_request(req_get_mem, manager.clone()).await;
        assert!(resp_get_mem.error.is_none());
        let result_get_mem = resp_get_mem.result.unwrap();
        let content_text_mem = result_get_mem["content"][0]["text"].as_str().unwrap();
        let val_mem: serde_json::Value = serde_json::from_str(content_text_mem).unwrap();
        assert_eq!(val_mem["status"], "success");
        assert_eq!(val_mem["memory"]["id"], memory_id);

        // 7c. Test tools/call - get_associations bidirectional
        let req_get_assoc_in = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(16)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_associations",
                "arguments": {
                    "source_id": exp_id,
                    "direction": "incoming"
                }
            })),
        };
        let resp_get_assoc_in = handle_request(req_get_assoc_in, manager.clone()).await;
        assert!(resp_get_assoc_in.error.is_none());
        let val_in: serde_json::Value = serde_json::from_str(
            resp_get_assoc_in.result.unwrap()["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(val_in["associations"].as_array().unwrap().len(), 1);
        assert_eq!(val_in["associations"][0]["source_id"], memory_id);
        assert_eq!(val_in["associations"][0]["target_id"], exp_id);

        Ok(())
    }

    #[tokio::test]
    async fn test_e2e_jsonrpc_edge_cases() -> Result<()> {
        let manager = Arc::new(Mutex::new(MemoryManager::new(":memory:")?));

        // 1. Tool call with missing parameters
        let req_missing_params = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(7)),
            method: "tools/call".to_string(),
            params: None,
        };
        let resp_missing = handle_request(req_missing_params, manager.clone()).await;
        assert!(resp_missing.error.is_some());
        assert_eq!(resp_missing.error.unwrap().code, -32602); // invalid params

        // 2. Unknown tool name
        let req_unknown_tool = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(8)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "non_existent_tool",
                "arguments": {}
            })),
        };
        let resp_unknown = handle_request(req_unknown_tool, manager.clone()).await;
        assert!(resp_unknown.error.is_none());
        let result_unknown = resp_unknown.result.unwrap();
        assert_eq!(result_unknown["isError"], true);
        assert!(result_unknown["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Tool 'non_existent_tool' not found"));

        // 3. Invalid layer string defaults to Session layer, which fails if session_id is missing
        let req_invalid_layer = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(9)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_memory",
                "arguments": {
                    "layer": "InvalidLayerEnumName",
                    "content": "Should default to Session layer and fail because no session_id is provided."
                }
            })),
        };
        let resp_invalid_layer = handle_request(req_invalid_layer, manager.clone()).await;
        assert!(resp_invalid_layer.error.is_none());
        let result_invalid = resp_invalid_layer.result.unwrap();
        assert_eq!(result_invalid["isError"], true);
        assert!(result_invalid["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Session ID is required"));

        // 4. Invalid layer string with session_id succeeds as Session layer
        let req_invalid_layer_with_id = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(10)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_memory",
                "arguments": {
                    "layer": "InvalidLayerEnumName",
                    "content": "Should default to Session layer and succeed because session_id is provided.",
                    "session_id": "session-xyz"
                }
            })),
        };
        let resp_invalid_layer_with_id =
            handle_request(req_invalid_layer_with_id, manager.clone()).await;
        assert!(resp_invalid_layer_with_id.error.is_none());
        let result_val = resp_invalid_layer_with_id.result.unwrap();
        let content_text = result_val["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        assert_eq!(val["status"], "success");

        // 5. Test store_memory with legacy "Principle" layer (backward compatibility check)
        let req_legacy_principle = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(13)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "store_memory",
                "arguments": {
                    "layer": "Principle",
                    "content": "Legacy rule memory stored under Principle label."
                }
            })),
        };
        let resp_legacy = handle_request(req_legacy_principle, manager.clone()).await;
        assert!(resp_legacy.error.is_none());
        let result_legacy = resp_legacy.result.unwrap();
        let content_text = result_legacy["content"][0]["text"].as_str().unwrap();
        let val: serde_json::Value = serde_json::from_str(content_text).unwrap();
        assert_eq!(val["status"], "success");
        let principle_id = val["memory_id"].as_str().unwrap().to_string();

        // Verify it was stored as Rule layer
        {
            let manager_lock = manager.lock().await;
            let entry = manager_lock
                .storage
                .get_memory_by_id(&principle_id)
                .unwrap()
                .unwrap();
            assert_eq!(entry.layer, models::MemoryLayer::Rule);
        }

        // 6. Test evaluate_experience on a non-Experience layer (should return error)
        let req_invalid_eval = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(14)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "evaluate_experience",
                "arguments": {
                    "memory_id": principle_id, // This is a Rule, not an Experience!
                    "success": true
                }
            })),
        };
        let resp_invalid_eval = handle_request(req_invalid_eval, manager.clone()).await;
        assert!(resp_invalid_eval.error.is_none()); // The MCP wrapper catches error and returns it in JSON body
        let result_eval = resp_invalid_eval.result.unwrap();
        assert_eq!(result_eval["isError"], true);
        assert!(result_eval["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Memory is not in the Experience layer"));

        Ok(())
    }

    #[test]
    fn test_parse_and_validate_request_logic() {
        // 1. Valid request
        let valid_json = r#"{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}"#;
        let res = parse_and_validate_request(valid_json);
        assert!(res.is_ok());
        let req = res.unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/list");

        // 2. Syntax Parse error (-32700)
        let invalid_json = r#"{"jsonrpc": "2.0", "id": 1, "method": "#;
        let res = parse_and_validate_request(invalid_json);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err["error"]["code"], -32700);

        // 3. Structural Invalid Request (-32600) - missing method
        let missing_method = r#"{"jsonrpc": "2.0", "id": 2}"#;
        let res = parse_and_validate_request(missing_method);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err["error"]["code"], -32600);
        assert_eq!(err["error"]["message"], "Invalid Request");
        assert_eq!(err["id"], 2);

        // 4. Structural Invalid Request (-32600) - missing jsonrpc
        let missing_jsonrpc = r#"{"method": "tools/list", "id": 3}"#;
        let res = parse_and_validate_request(missing_jsonrpc);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err["error"]["code"], -32600);
        assert_eq!(err["id"], 3);
    }

    #[tokio::test]
    async fn test_adversarial_jsonrpc_error_handling() -> Result<()> {
        let manager = Arc::new(Mutex::new(MemoryManager::new(":memory:")?));

        // 1. Structural Invalid Request (-32600) - incorrect version
        let bad_version = r#"{"jsonrpc": "1.0", "method": "tools/list", "id": 100}"#;
        let res = parse_and_validate_request(bad_version);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err["error"]["code"], -32600);
        assert_eq!(err["id"], 100);

        // 2. Structural Invalid Request (-32600) - non-string method
        let non_string_method = r#"{"jsonrpc": "2.0", "method": 123, "id": 101}"#;
        let res2 = parse_and_validate_request(non_string_method);
        assert!(res2.is_err());
        let err2 = res2.unwrap_err();
        assert_eq!(err2["error"]["code"], -32600);

        // 3. Structural Invalid Request (-32600) - null method
        let null_method = r#"{"jsonrpc": "2.0", "method": null, "id": 102}"#;
        let res3 = parse_and_validate_request(null_method);
        assert!(res3.is_err());
        let err3 = res3.unwrap_err();
        assert_eq!(err3["error"]["code"], -32600);

        // 4. Structural Invalid Request (-32600) - array instead of object
        let array_request = r#"[{"jsonrpc": "2.0", "method": "tools/list", "id": 103}]"#;
        let res4 = parse_and_validate_request(array_request);
        assert!(res4.is_err());
        let err4 = res4.unwrap_err();
        assert_eq!(err4["error"]["code"], -32600);

        // 5. Structural Invalid Request (-32600) - integer instead of object
        let int_request = r#"12345"#;
        let res5 = parse_and_validate_request(int_request);
        assert!(res5.is_err());
        let err5 = res5.unwrap_err();
        assert_eq!(err5["error"]["code"], -32600);

        // 6. Unknown tool name does not yield -32601 but isError: true standard tool response
        let req_unknown_tool = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(200)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "unrecognized_adversarial_tool",
                "arguments": {}
            })),
        };
        let resp_unknown = handle_request(req_unknown_tool, manager.clone()).await;
        assert!(
            resp_unknown.error.is_none(),
            "Unrecognized tool calls must not yield JSON-RPC error"
        );
        let result_unknown = resp_unknown.result.unwrap();
        assert_eq!(
            result_unknown["isError"], true,
            "Unrecognized tool must yield isError: true"
        );
        assert!(
            result_unknown["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Tool 'unrecognized_adversarial_tool' not found"),
            "Should return 'not found' text content"
        );

        Ok(())
    }
}
