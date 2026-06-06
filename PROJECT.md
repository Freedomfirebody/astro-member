# Workspace Architecture Design & Specification

## Architecture
We are refactoring the `astro-member` project into a multi-crate Cargo Workspace. This decouples the core storage and search engine from the MCP communication protocol.

### Crate 1: `astro-member` (Core Library)
- **Path**: `core/`
- **Type**: Library (`lib.rs`)
- **Responsibility**: All memory storage logic (SQLite engine, database migrations, TF-IDF and semantic vector search, embedding generator, experience reinforcement engine).
- **Public API**:
  - `MemoryManager` struct with methods: `new`, `store`, `retrieve`, `evaluate_experience`, `get_memory_by_id`, `get_conflict_candidates`, `resolve_conflict`, `get_session_memories`, `purge_session_memories`, `create_association`, `get_associations`.
  - All database models (`MemoryEntry`, `Association`, `SearchResult`, `ConflictCandidate`, etc.).
- **Constraints**: No stdin/stdout loops, no JSON-RPC parser, no MCP schemas.

### Crate 2: `astro-member-mcp` (MCP Server Binary)
- **Path**: `mcp/`
- **Type**: Binary (`main.rs`)
- **Responsibility**: Stdin/stdout command loop, JSON-RPC 2.0 request parsing and routing, MCP schemas definition and validation, calling core library APIs.
- **Dependency**: Paths to the `astro-member` core library.

---

## Code Layout
```
astro-member/ (workspace root)
├── Cargo.toml (workspace definition)
├── PROJECT.md (this file)
├── core/
│   ├── Cargo.toml (library crate configuration)
│   └── src/
│       ├── lib.rs (exports the modules)
│       ├── embedding.rs
│       ├── evolution.rs
│       ├── memory_manager.rs
│       ├── models.rs
│       ├── search.rs
│       ├── storage.rs
│       └── tfidf_search.rs
├── mcp/
│   ├── Cargo.toml (binary crate configuration)
│   └── src/
│       └── main.rs (contains stdin/stdout loop and JSON-RPC routing)
├── tests/ (optional integration tests)
├── local_release.bat (release script building and copying to C:\Development\Warehouse\Mem-mcp)
└── test_e2e.py (92+ Python E2E integration tests)
```

---

## Interface Contracts

### Binary `astro-member-mcp` ↔ Library `astro-member`
The binary calls the library in-process via public Rust APIs on `MemoryManager`.

```rust
// In core/src/lib.rs:
pub mod embedding;
pub mod evolution;
pub mod memory_manager;
pub mod models;
pub mod search;
pub mod storage;
pub mod tfidf_search;
```

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | Create Workspace Layout | Create root `Cargo.toml`, folders `core/` and `mcp/`, copy/move source files, create `PROJECT.md` | none | COMPLETED |
| 2 | Core Crate Setup | Configure `core/Cargo.toml`, adapt code in `core/src/` to be a library (`lib.rs`), ensure compilation | M1 | COMPLETED |
| 3 | MCP Crate Setup | Configure `mcp/Cargo.toml`, move JSON-RPC loop and models into `mcp/src/main.rs`, ensure compilation | M2 | COMPLETED |
| 4 | Integration Tests Setup | Move/adapt tests, ensure workspace unit/integration tests run and pass | M3 | COMPLETED |
| 5 | Update Release Script | Update `local_release.bat` to build the workspace and publish compiled binary to `C:\Development\Warehouse\Mem-mcp\astro-member.exe` | M4 | COMPLETED |
| 6 | E2E Python Testing | Run `pytest test_e2e.py` against the new release binary, verify 92+ tests pass | M5 | COMPLETED |
