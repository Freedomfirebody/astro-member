# Astro-Member: Hierarchical Memory MCP Server

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![MCP](https://img.shields.io/badge/protocol-Model%20Context%20Protocol-blue.svg)](https://modelcontextprotocol.io/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#license)

`astro-member` is a lightweight, high-performance **Model Context Protocol (MCP) Server** written in Rust. It serves as an external memory system for LLM agents, featuring **hierarchical memory layers**, **graph semantic associations**, **temporal exponential decay**, **experience reinforcement**, and **hybrid vector/keyword search**.

Instead of relying on heavy vector database servers, `astro-member` utilizes a localized SQLite database alongside an embedded embedding model (`fastembed`) to achieve self-contained, low-latency semantic indexing and relationship storage.

---

## 🗺️ System Architecture

The server handles incoming JSON-RPC 2.0 requests over standard I/O (stdin/stdout) and coordinates embedding extraction, SQLite storage, decay calculations, and hybrid similarity ranking.

```mermaid
graph TD
    User([User Request]) --> Client[LLM Client / Claude Desktop]
    Client -->|JSON-RPC 2.0 over Stdin| Main[src/main.rs]
    
    subgraph Astro-Member Server
        Main <-->|Session / Params| MM[src/memory_manager.rs]
        MM <-->|Store & Query| Storage[src/storage.rs]
        MM <-->|Generate Embeddings| Embedding[src/embedding.rs]
        
        Storage <-->|SQLite Local File| DB[(memory.db)]
        Embedding <-->|Embedded ONNX| FE[FastEmbed Cache]
        
        MM -->|Retrieve Candidates| Search[src/search.rs]
        Search -->|Vector Cosine Similarity| Dense[Dense Search]
        Search -->|BM25 Keyword Scoring| BM25[src/tfidf_search.rs]
        
        Search -->|Apply Exponential Decay| Decay[Temporal Decay Engine]
        Decay -->|Evaluate Experiences| Evolution[src/evolution.rs]
        
        Dense & BM25 & Decay & Evolution -->|Cohesive Weighting & Limit| Ranker[Ranking Engine]
    end
    
    Ranker -->|Ranked Context| Main
    Main -->|JSON-RPC 2.0 over Stdout| Client
```

---

## ✨ Core Features

### 1. Hierarchical Memory Layers
Memory is organized into four logical layers, each with distinct retention characteristics, base weights, and decay rules:

*   **Rule Layer (highest priority, base weight: `2.0`)**: 
    Permanent instructions and core guidelines. Completely exempt from relevance filtering and temporal decay.
*   **Persona Layer (medium-high priority, base weight: `1.5`)**: 
    Captures user/agent tone, preference, and style. Decays extremely slowly ($e^{-0.001t}$).
*   **Experience Layer (medium priority, base weight: `1.0`)**: 
    Stores problem-solving strategies, designs, and historical scenarios. Score can be dynamically reinforced (multiplied by `1.1` on success, `0.8` on failure, clamped between `0.1` and `5.0`).
*   **Session Layer (standard priority, base weight: `1.0`)**: 
    Short-term contextual memory isolated by `session_id`. Decays rapidly ($e^{-0.2t}$) to mimic human forgetting.

### 2. File-Based Graph Semantic Associations
Connects discrete memories with directed, typed semantic relations (e.g. `depends_on`, `related_to`, `contradicts`).
*   Strict validation prevents self-referential relations or connections to non-existent nodes.
*   Supports querying incoming, outgoing, or bidirectional associations.
*   **Cascading Deletion**: Deleting a memory automatically purges all connected associations in the database to maintain referential integrity.

### 3. Dual-Track Hybrid Search Engine
*   **Dense Search**: Embedded `fastembed` (BGEM3) extracts vector representations. Retrieves matches using Cosine Similarity with automatic clamping against NaN or zero-norm anomalies.
*   **Sparse Search**: Custom in-memory BM25-based TF-IDF algorithm tracks word frequencies to guarantee high-scoring exact keyword matching.
*   **Bypass Exemption**: Rule/Principle layers bypass the minimum relevance score gate (`0.15`), ensuring critical rules are always active in context.

---

## 🛠️ MCP Tool Interface API

The server exposes the following 6 core JSON-RPC tools to the client:

### `store_memory`
Stores a memory in the database under a specified layer.
*   **Arguments**:
    *   `layer` (string, enum: `["Rule", "Persona", "Experience", "Session"]`): Target layer.
    *   `content` (string): The memory text.
    *   `session_id` (string, optional): Required if `layer` is `"Session"`.
    *   `context_tags` (array of strings, optional): Arbitrary tags for manual filtering.

### `retrieve_memory`
Performs a hybrid semantic search across layers, applying temporal decay and session isolation.
*   **Arguments**:
    *   `query` (string): The search query.
    *   `session_id` (string, optional): Restricts session search to this specific ID.

### `get_memory_by_id`
Retrieves a specific memory by its unique ID.
*   **Arguments**:
    *   `id` (string): The UUID of the memory.
    *   `session_id` (string, optional): Required if retrieving a session memory to ensure isolation bounds.

### `evaluate_experience`
Performs reinforcement learning by providing feedback on an Experience memory.
*   **Arguments**:
    *   `memory_id` (string): The ID of the Experience memory.
    *   `success` (boolean): `true` boosts the evaluation weight; `false` penalizes it.

### `create_association`
Draws a directed graph association between two memories.
*   **Arguments**:
    *   `source_id` (string): The source memory UUID.
    *   `target_id` (string): The target memory UUID.
    *   `relation_type` (string): The label representing the relation.

### `get_associations`
Queries relations connected to a source memory.
*   **Arguments**:
    *   `source_id` (string): The source memory UUID.
    *   `direction` (string, enum: `["outgoing", "incoming", "both"]`, optional): Direction of relations to traverse. Defaults to `"outgoing"`.

---

## 🚀 Getting Started

### Prerequisites

*   **Rust**: Stable toolchain (Edition 2021)
*   **Python 3.8+** (Optional, for running E2E test suite)

### Compilation

Build the release binary:
```bash
cargo build --release
```
The compiled executable will be located at `target/release/astro-member.exe` (on Windows) or `target/release/astro-member` (on macOS/Linux).

### Integration with Claude Desktop

Add `astro-member` as an MCP server by modifying your `claude_desktop_config.json` configuration file:

*   **Windows (PowerShell)**: `%APPDATA%\Claude\claude_desktop_config.json`
*   **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`

Add the following config (adjust the executable path to point to your target directory):

```json
{
  "mcpServers": {
    "astro-member": {
      "command": "d:\\Project\\AiProject\\astro-member\\target\\release\\astro-member.exe",
      "args": []
    }
  }
}
```

Restart Claude Desktop, and you will see the `astro-member` tool icon appear in the chat input area.

---

## 🧪 Testing & Verification

We maintain a high standard of coverage consisting of unit tests and a comprehensive Python-based E2E test suite.

### Running Unit Tests
Executes database transaction, model mapping, and in-memory TF-IDF tests:
```bash
cargo test
```

### Running E2E Test Suite
The E2E test suite (`test_e2e.py`) launches the compiled binary in an isolated temp folder, sends JSON-RPC 2.0 payloads via stdin, reads stdout, and verifies behaviour against Tiers 1-4 (82 tests total).

1.  Compile the debug binary:
    ```bash
    cargo build
    ```
2.  Install `pytest`:
    ```bash
    pip install pytest
    ```
3.  Run the tests:
    ```bash
    pytest test_e2e.py -v
    ```

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
