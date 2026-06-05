# Hierarchical Memory MCP (Rust)

A lightweight but highly efficient, file-based Memory Model Context Protocol (MCP) server. 
It avoids heavy external vector databases or graph databases in favor of an optimized local BM25 textual retrieval approach coupled with complex memory hierarchy rules.

## Features Supported & Target Architecture

1. **Hierarchical Memory Architecture**:
   - **Principle Layer**: Highest base weight (10.0), no decay. (Rules, ethics).
   - **Persona Layer**: Weight (5.0), extremely slow decay rate. (Style, interaction prefs).
   - **Experience Layer**: Weight (3.0), average decay rate. (Experiences and contextual strategies).
   - **Session Layer**: Weight (1.0), fast decay rate. (Current working memory context, STRICTLY isolated by `session_id`).

2. **No Heavy Vector Databases**:
   Instead of generating and storing float 32 vectors in an external DB, it uses an internal `LightweightSearch` (BM25 adaptation) algorithm for fast textual relevancy matching across active context tags and contents.

3. **Memory Inner Cohesion**:
   Saved locally inside `.mcp_memory_storage` using independent JSON files (`{uuid}.json`). This naturally segregates memory to avoid cross-pollution, allows for discrete isolation reads, and leverages standard file I/O for graph-like tag relations.

4. **Experience Assessment Mechanisms**:
   It supports a `evaluate_experience` MCP tool call. When an agent succeeds or fails at a task, it adjusts the `evaluation_score` multiplier of that specific memory, ensuring successful patterns surface higher in the future.

5. **Decay Mechanics**:
   `final_score = text_score * layer_weight * decay * evaluation_score`.
   The decay is dynamically evaluated at retrieval time, applying e^-(decay_rate * days_old) calculation.

## How to Build & Run

Ensure you have Rust and Cargo installed (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`).

```bash
# Build the project
cargo build --release

# Run the MCP STDIO Server
cargo run --release
```

## Adding to Cursor / Claude

Point your Desktop applications to run the compiled binary via `stdio`:

```json
{
  "mcpServers": {
    "hierarchical-memory": {
      "command": "/path/to/your/project/target/release/hierarchical-memory-mcp",
      "args": []
    }
  }
}
```
