# RFC: Memory Evolution Features for Astro-Member

This RFC defines the schemas and workflows for the astro-member memory server, introducing semantic conflict resolution, dynamic memory compaction (rolling summarization), and windowed context management.

---

## 1. JSON-RPC 2.0 Schemas for New Tools

All tool executions are invoked through the standard MCP tool execution method: `tools/call` or `mcp.tools.call`.

### 1.1 `get_conflict_candidates`
Identifies active memories that are semantically close to a proposed memory entry.

*   **Name**: `get_conflict_candidates`
*   **Description**: "Find active memories that are semantically similar to the proposed content."
*   **Input Schema (JSON Schema)**:
    ```json
    {
      "type": "object",
      "properties": {
        "content": { "type": "string", "description": "The proposed new memory content." },
        "session_id": { "type": "string", "description": "Strictly scope search to this session (optional)." },
        "threshold": { "type": "number", "description": "Minimum similarity score to qualify as a conflict. Defaults to 0.70." },
        "limit": { "type": "integer", "description": "Max candidates to return. Defaults to 5." }
      },
      "required": ["content"]
    }
    ```
*   **Output Result Format**:
    ```json
    {
      "candidates": [
        {
          "memory": {
            "id": "mem-id-uuid",
            "layer": "Experience",
            "session_id": null,
            "content": "Existing similar content",
            "tags": ["testing"],
            "created_at": "2026-06-06T12:00:00Z",
            "last_accessed": "2026-06-06T12:30:00Z",
            "access_count": 2,
            "evaluation_score": 1.0
          },
          "similarity": 0.85
        }
      ]
    }
    ```

### 1.2 `resolve_conflict`
Performs multi-step conflict resolution operations (deprecations, deletes, inserts, and associations) atomically in a single database transaction.

*   **Name**: `resolve_conflict`
*   **Description**: "Atomically execute a set of deprecations, deletions, memory insertions, and association updates to resolve conflicts."
*   **Input Schema (JSON Schema)**:
    ```json
    {
      "type": "object",
      "properties": {
        "deprecate_ids": {
          "type": "array",
          "items": { "type": "string" },
          "description": "IDs of existing memories to soft-deprecate (status set to Deprecated)."
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
              "layer": { "type": "string", "enum": ["Rule", "Persona", "Experience", "Session"] },
              "session_id": { "type": "string", "description": "Required if layer is Session." },
              "content": { "type": "string" },
              "context_tags": { "type": "array", "items": { "type": "string" } }
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
              "source_id": { "type": "string", "description": "The ID of the source memory node." },
              "target_id": { "type": "string", "description": "The ID of the target memory node." },
              "relation_type": { "type": "string", "description": "E.g., 'replaces', 'contradicts', 'supersedes'." }
            },
            "required": ["source_id", "target_id", "relation_type"]
          },
          "description": "Graph associations to write connecting new or existing memories."
        }
      }
    }
    ```
*   **Output Result Format**:
    ```json
    {
      "status": "success",
      "inserted_ids": ["new-uuid-1", "new-uuid-2"]
    }
    ```

### 1.3 `get_session_memories`
Exposes the chronological timeline of active memories inside a specific session for summarization/rolling context compilation.

*   **Name**: `get_session_memories`
*   **Description**: "Retrieve all active memories associated with a given session in chronological order."
*   **Input Schema (JSON Schema)**:
    ```json
    {
      "type": "object",
      "properties": {
        "session_id": { "type": "string", "description": "The target session ID." },
        "limit": { "type": "integer", "description": "Max memories to retrieve (optional)." }
      },
      "required": ["session_id"]
    }
    ```
*   **Output Result Format**:
    ```json
    {
      "memories": [
        {
          "id": "session-mem-1",
          "layer": "Session",
          "session_id": "session-123",
          "content": "User logged in.",
          "tags": ["auth"],
          "created_at": "2026-06-06T12:00:00Z",
          "last_accessed": "2026-06-06T12:00:00Z",
          "access_count": 1,
          "evaluation_score": 1.0
        }
      ]
    }
    ```

### 1.4 `purge_session_memories`
Clears out granular session history records, either soft-deprecating or permanently deleting them, with a safeguard to preserve specific summary records.

*   **Name**: `purge_session_memories`
*   **Description**: "Bulk soft-deprecate or hard-delete memories in a session, preserving specified items."
*   **Input Schema (JSON Schema)**:
    ```json
    {
      "type": "object",
      "properties": {
        "session_id": { "type": "string", "description": "The session ID to purge." },
        "preserve_ids": {
          "type": "array",
          "items": { "type": "string" },
          "description": "IDs to protect from being purged (e.g. the newly written summary)."
        },
        "permanent": {
          "type": "boolean",
          "description": "If true, permanently delete. If false, soft-deprecate by changing status to 'Deprecated'. Defaults to false."
        }
      },
      "required": ["session_id"]
    }
    ```
*   **Output Result Format**:
    ```json
    {
      "status": "success",
      "purged_count": 12
    }
    ```

---

## 2. Windowed Context Management Schema Updates

The `retrieve_memory` tool is updated to return advanced layout details, helping the client compute dynamic context windows without server-side hardcoding of character/token budgets.

### 2.1 `retrieve_memory` Output Update
The `results` array elements (originally containing only `memory` and `final_score`) are extended.

*   **Extended `SearchResult` Structure**:
    ```json
    {
      "results": [
        {
          "memory": {
            "id": "rule-1",
            "layer": "Rule",
            "session_id": null,
            "content": "Always be polite.",
            "tags": ["general"],
            "created_at": "2026-06-06T10:00:00Z",
            "last_accessed": "2026-06-06T13:00:00Z",
            "access_count": 5,
            "evaluation_score": 1.0
          },
          "final_score": 10.0,
          "size": 17,
          "created_at": "2026-06-06T10:00:00Z",
          "cumulative_size": 17
        },
        {
          "memory": {
            "id": "session-1",
            "layer": "Session",
            "session_id": "session-abc",
            "content": "User prefers Rust.",
            "tags": ["language"],
            "created_at": "2026-06-06T11:00:00Z",
            "last_accessed": "2026-06-06T13:00:00Z",
            "access_count": 1,
            "evaluation_score": 1.0
          },
          "final_score": 1.0,
          "size": 18,
          "created_at": "2026-06-06T11:00:00Z",
          "cumulative_size": 35
        }
      ]
    }
    ```

---

## 3. Client-Side Orchestration Workflows

### 3.1 Conflict Check & Resolution Loop
When an Agent attempts to store a new memory (e.g. after a task is finished):
1.  **Detect**: Call `get_conflict_candidates` with the new content, optional `session_id`, and a confidence `threshold`.
2.  **Evaluate**:
    *   If no candidates are returned, proceed to call `store_memory` as usual.
    *   If candidates are returned, the Agent parses their content.
    *   For each candidate, the Agent's LLM determines if it is:
        *   *Contradictory*: The old memory is incorrect/outdated and must be deprecated.
        *   *Redundant*: The new memory is already fully covered by the old memory (no-op).
        *   *Complementary/Additive*: Both should be kept, possibly linked.
3.  **Resolve**: Invoke `resolve_conflict` containing:
    *   `deprecate_ids`: IDs of outdated candidates.
    *   `new_memories`: The new memory.
    *   `new_associations`: An association of type `"replaces"` or `"supersedes"` between the new memory and the deprecated memory.

### 3.2 Compaction & Rolling Summarization Loop
To keep the session context window small, the client monitors the session memory size:
1.  **Monitor**: Periodically fetch active memories in the current session via `get_session_memories(session_id)`.
2.  **Trigger**: If the count of session memories exceeds a threshold (e.g. 15 entries) or cumulative size is too large:
    *   The Agent sends the chronological list of session memory contents to the LLM.
    *   The LLM generates a consolidated rolling summary of the session.
3.  **Consolidate**:
    *   Call `store_memory` to write the new summary under the `Session` layer (with tag `"summary"`), which returns `summary_id`.
    *   Call `purge_session_memories(session_id, preserve_ids=[summary_id], permanent=false)`.
4.  All granular records are soft-deprecated, leaving only the consolidated summary active in the retrieve context.
