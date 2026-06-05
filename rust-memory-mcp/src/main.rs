pub mod memory_manager;
pub mod models;
pub mod tfidf_search;

use anyhow::Result;
use models::{McpRequest, McpResponse, JsonRpcError};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::sync::Mutex;
use memory_manager::MemoryManager;

#[tokio::main]
async fn main() -> Result<()> {
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

        let req: Result<McpRequest, _> = serde_json::from_str(&line);
        match req {
            Ok(request) => {
                let response = handle_request(request, manager.clone()).await;
                let serialized = serde_json::to_string(&response)?;
                writeln!(stdout, "{}", serialized)?;
                stdout.flush()?;
            }
            Err(e) => {
                // Invalid JSON
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                });
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
        "mcp.server.info" => {
            Ok(json!({
                "name": "hierarchical-memory-mcp",
                "version": "0.1.0",
                "capabilities": {
                    "tools": true
                }
            }))
        }
        "mcp.tools.list" => {
            Ok(json!({
                "tools": [
                    {
                        "name": "store_memory",
                        "description": "Store a memory conceptually into different hierarchical layers.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "layer": { "type": "string", "enum": ["Principle", "Persona", "Experience", "Session"] },
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
                        "parameters": {
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
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "memory_id": { "type": "string" },
                                "success": { "type": "boolean" },
                                "feedback": { "type": "string" }
                            },
                            "required": ["memory_id", "success"]
                        }
                    }
                ]
            }))
        }
        "mcp.tools.call" => {
            if let Some(params) = req.params {
                let name = params["name"].as_str().unwrap_or("");
                let args = &params["arguments"];

                match name {
                    "store_memory" => {
                        let layer_str = args["layer"].as_str().unwrap_or("Session");
                        let layer = match layer_str {
                            "Principle" => models::MemoryLayer::Principle,
                            "Persona" => models::MemoryLayer::Persona,
                            "Experience" => models::MemoryLayer::Experience,
                            _ => models::MemoryLayer::Session,
                        };
                        let session_id = args["session_id"].as_str().map(String::from);
                        let content = args["content"].as_str().unwrap_or("").to_string();
                        let tags: Vec<String> = args["context_tags"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();

                        match mgr.store(layer, session_id, content, tags) {
                            Ok(id) => Ok(json!({ "status": "success", "memory_id": id })),
                            Err(e) => Err(JsonRpcError::internal(e.to_string())),
                        }
                    }
                    "retrieve_memory" => {
                        let query = args["query"].as_str().unwrap_or("");
                        let session_id = args["session_id"].as_str().map(String::from);

                        match mgr.retrieve(query, session_id) {
                            Ok(results) => Ok(json!({ "results": results })),
                            Err(e) => Err(JsonRpcError::internal(e.to_string())),
                        }
                    }
                    "evaluate_experience" => {
                        let memory_id = args["memory_id"].as_str().unwrap_or("");
                        let success = args["success"].as_bool().unwrap_or(false);
                        
                        match mgr.evaluate_experience(memory_id, success) {
                            Ok(_) => Ok(json!({ "status": "evaluated" })),
                            Err(e) => Err(JsonRpcError::internal(e.to_string())),
                        }
                    }
                    _ => Err(JsonRpcError::method_not_found()),
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
