import pytest
import os
import sys
import json
import time
import sqlite3
import tempfile
import subprocess
import struct
import threading
from datetime import datetime, timedelta

def parse_iso_datetime(dt_str):
    # Remove 'Z' if present
    dt_str = dt_str.rstrip('Z')
    # If there's a dot, truncate the fractional part to 6 digits (microseconds)
    if '.' in dt_str:
        parts = dt_str.split('.')
        if len(parts) == 2:
            base, frac = parts
            dt_str = f"{base}.{frac[:6]}"
    return datetime.fromisoformat(dt_str)

current_dir = os.path.dirname(os.path.abspath(__file__))
SERVER_BINARY = os.path.join(current_dir, "target", "debug", "astro-member.exe")

class ServerInstance:
    def __init__(self, temp_dir, fastembed_cache_path=None):
        self.temp_dir = temp_dir
        self.fastembed_cache_path = fastembed_cache_path
        self.db_dir = os.path.join(temp_dir, ".mcp_memory_storage")
        self.db_path = os.path.join(self.db_dir, "memory.db")
        self.process = None
        self.stderr_log = None
        self.id_counter = 1
        self.lock = threading.Lock()
        self.start()

    def start(self):
        os.makedirs(self.temp_dir, exist_ok=True)
        env = os.environ.copy()
        if self.fastembed_cache_path is not None:
            env["FASTEMBED_CACHE_PATH"] = self.fastembed_cache_path
        else:
            env["FASTEMBED_CACHE_PATH"] = os.path.join(current_dir, ".fastembed_cache")
        self.stderr_log = open(os.path.join(self.temp_dir, "stderr.log"), "a+b")
        self.process = subprocess.Popen(
            [SERVER_BINARY],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr_log,
            cwd=self.temp_dir,
            env=env,
            bufsize=0
        )
        init_res = self.call("initialize", {})
        assert "protocolVersion" in init_res, f"Init failed: {init_res}"
        self.notify("notifications/initialized", {})

    def stop(self):
        if self.process:
            self.process.stdin.close()
            try:
                self.process.terminate()
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process = None
        if self.stderr_log:
            self.stderr_log.close()
            self.stderr_log = None

    def call(self, method, params):
        with self.lock:
            req_id = self.id_counter
            self.id_counter += 1
            request = {
                "jsonrpc": "2.0",
                "id": req_id,
                "method": method,
                "params": params
            }
            req_str = json.dumps(request) + "\n"
            self.process.stdin.write(req_str.encode('utf-8'))
            self.process.stdin.flush()

            line = self.process.stdout.readline()
            if not line:
                if self.stderr_log:
                    self.stderr_log.seek(0)
                    stderr_data = self.stderr_log.read()
                    raise RuntimeError(f"Server exited or closed pipe. Stderr: {stderr_data.decode('utf-8')}")
                else:
                    raise RuntimeError("Server exited or closed pipe.")
            resp = json.loads(line.decode('utf-8'))
            assert resp.get("jsonrpc") == "2.0", f"Invalid JSONRPC version: {resp}"
            assert resp.get("id") == req_id, f"ID mismatch: expected {req_id}, got {resp.get('id')}"
            if "error" in resp:
                return resp
            return resp["result"]

    def notify(self, method, params):
        with self.lock:
            request = {
                "jsonrpc": "2.0",
                "method": method,
                "params": params
            }
            req_str = json.dumps(request) + "\n"
            self.process.stdin.write(req_str.encode('utf-8'))
            self.process.stdin.flush()

    def call_tool(self, name, arguments):
        return self.call("tools/call", {
            "name": name,
            "arguments": arguments
        })

    def direct_db_query(self, query, params=()):
        self.stop()
        conn = sqlite3.connect(self.db_path)
        conn.execute("PRAGMA foreign_keys = ON;")
        cursor = conn.cursor()
        cursor.execute(query, params)
        res = cursor.fetchall()
        conn.close()
        self.start()
        return res

    def direct_db_update(self, query, params=()):
        self.stop()
        conn = sqlite3.connect(self.db_path)
        conn.execute("PRAGMA foreign_keys = ON;")
        cursor = conn.cursor()
        cursor.execute(query, params)
        conn.commit()
        conn.close()
        self.start()

def parse_tool_result(res):
    if "isError" in res and res["isError"]:
        # Return False and the error message
        return False, res["content"][0]["text"]
    text = res["content"][0]["text"]
    try:
        return True, json.loads(text)
    except json.JSONDecodeError:
        return True, text

@pytest.fixture
def server():
    temp_dir = tempfile.mkdtemp()
    srv = ServerInstance(temp_dir)
    yield srv
    srv.stop()
    # Clean up temp_dir manually, ignoring permission/lock errors on Windows
    import shutil
    for _ in range(5):
        try:
            shutil.rmtree(temp_dir)
            break
        except Exception:
            time.sleep(0.1)

# ==========================================
# SUITE 1: Session Isolation (5 cases)
# ==========================================

def test_1_1_store_session_memory_requires_session_id(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "content": "User is debugging Python script."
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Session ID is required" in data

def test_1_2_global_layer_stores_cannot_accept_session_id(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "session_id": "sess-1",
        "content": "Concise responses."
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Session ID must not be provided" in data

def test_1_3_absolute_visibility_isolation_between_sessions(server):
    res_a = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-A",
        "content": "User prefers dark mode UI."
    })
    assert parse_tool_result(res_a)[0]
    
    res_b = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-B",
        "content": "User prefers light mode UI."
    })
    assert parse_tool_result(res_b)[0]

    # Session A retrieve
    ret_a = server.call_tool("retrieve_memory", {
        "query": "prefers UI",
        "session_id": "session-A"
    })
    results_a = parse_tool_result(ret_a)[1]["results"]
    assert len(results_a) == 1
    assert results_a[0]["memory"]["content"] == "User prefers dark mode UI."

    # Session B retrieve
    ret_b = server.call_tool("retrieve_memory", {
        "query": "prefers UI",
        "session_id": "session-B"
    })
    results_b = parse_tool_result(ret_b)[1]["results"]
    assert len(results_b) == 1
    assert results_b[0]["memory"]["content"] == "User prefers light mode UI."

def test_1_4_global_layers_visible_across_all_sessions(server):
    res_rule = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Always answer politely."
    })
    assert parse_tool_result(res_rule)[0]
    
    res_sess = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-123",
        "content": "User is coding in C#."
    })
    assert parse_tool_result(res_sess)[0]

    # Retrieve with session-123
    ret1 = server.call_tool("retrieve_memory", {"query": "answer politely", "session_id": "session-123"})
    assert len(parse_tool_result(ret1)[1]["results"]) == 1

    # Retrieve with session-other
    ret2 = server.call_tool("retrieve_memory", {"query": "answer politely", "session_id": "session-other"})
    assert len(parse_tool_result(ret2)[1]["results"]) == 1

    # Retrieve with no session
    ret3 = server.call_tool("retrieve_memory", {"query": "answer politely"})
    assert len(parse_tool_result(ret3)[1]["results"]) == 1

def test_1_5_no_session_retrieval_returns_empty_session_context(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-abc",
        "content": "User is a software architect."
    })
    assert parse_tool_result(res)[0]

    ret = server.call_tool("retrieve_memory", {"query": "software architect"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 0

# ==========================================
# SUITE 2: Rule Permanence (5 cases)
# ==========================================

def test_2_1_low_relevance_bypass_exemption(server):
    res_rule = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Rules of Engagement: Standard procedures apply."
    })
    assert parse_tool_result(res_rule)[0]

    res_sess = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-x",
        "content": "User likes eating organic apples."
    })
    assert parse_tool_result(res_sess)[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "banana pineapple grapes",
        "session_id": "session-x"
    })
    results = parse_tool_result(ret)[1]["results"]
    
    rule_results = [r for r in results if r["memory"]["layer"] == "Rule"]
    sess_results = [r for r in results if r["memory"]["layer"] == "Session"]
    assert len(rule_results) == 1
    assert len(sess_results) == 0

def test_2_2_principle_layer_alias_compatibility(server):
    res = server.call_tool("store_memory", {
        "layer": "Principle",
        "content": "System guidelines for safety."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    res_get = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(res_get)[1]["memory"]["layer"] == "Rule"

def test_2_3_retrieve_priority_dominance_base_weight(server):
    content = "The key concept is abstraction."
    
    assert parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": content}))[0]
    assert parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": content}))[0]
    assert parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": content}))[0]
    assert parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "session-1", "content": content}))[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "concept abstraction",
        "session_id": "session-1"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 4
    assert results[0]["memory"]["layer"] == "Rule"
    assert results[1]["memory"]["layer"] == "Persona"
    assert results[2]["memory"]["layer"] == "Experience"
    assert results[3]["memory"]["layer"] == "Session"

def test_2_4_permanent_rule_score_stability(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Safety first."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    ret1 = server.call_tool("retrieve_memory", {"query": "Safety first"})
    score1 = parse_tool_result(ret1)[1]["results"][0]["final_score"]

    target_time = (datetime.utcnow() - timedelta(days=30)).isoformat() + "Z"
    server.direct_db_update(
        "UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?",
        (target_time, mem_id)
    )

    ret2 = server.call_tool("retrieve_memory", {"query": "Safety first"})
    score2 = parse_tool_result(ret2)[1]["results"][0]["final_score"]

    assert abs(score1 - score2) < 1e-9

def test_2_5_rules_exist_in_global_memory_space_without_session_id(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Standard guidelines."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    res_get = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(res_get)[1]["memory"].get("session_id") is None

# ==========================================
# SUITE 3: Persona Adaptation (5 cases)
# ==========================================

def test_3_1_persona_slow_decay_factor(server):
    content = "User profile: Prefers python language."
    
    pers_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": content}))[1]["memory_id"]
    sess_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "session-p", "content": content}))[1]["memory_id"]

    ret_base = server.call_tool("retrieve_memory", {"query": "prefers python", "session_id": "session-p"})
    results_base = parse_tool_result(ret_base)[1]["results"]
    
    base_pers = next(r for r in results_base if r["memory"]["id"] == pers_id)["final_score"]
    base_sess = next(r for r in results_base if r["memory"]["id"] == sess_id)["final_score"]

    target_time = (datetime.utcnow() - timedelta(days=10)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, pers_id))
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, sess_id))

    ret_decay = server.call_tool("retrieve_memory", {"query": "prefers python", "session_id": "session-p"})
    results_decay = parse_tool_result(ret_decay)[1]["results"]
    
    decay_pers = next(r for r in results_decay if r["memory"]["id"] == pers_id)["final_score"]
    decay_sess = next(r for r in results_decay if r["memory"]["id"] == sess_id)["final_score"]

    pers_ratio = decay_pers / base_pers
    sess_ratio = decay_sess / base_sess

    assert abs(pers_ratio - 0.99) < 0.05
    assert abs(sess_ratio - 0.135) < 0.05

def test_3_2_persona_subject_to_relevance_filter(server):
    assert parse_tool_result(server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "Persona attribute: Enjoys baking cookies."
    }))[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "industrial nuclear reactors space engineering"
    })
    assert len(parse_tool_result(ret)[1]["results"]) == 0

def test_3_3_persona_retrieval_and_score_calculation(server):
    content = "Enjoys writing rust systems."
    pers_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": content}))[1]["memory_id"]
    rule_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": content}))[1]["memory_id"]

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (pers_id,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (rule_id,))

    ret = server.call_tool("retrieve_memory", {"query": "writing rust systems"})
    results = parse_tool_result(ret)[1]["results"]

    score_rule = next(r for r in results if r["memory"]["id"] == rule_id)["final_score"]
    score_pers = next(r for r in results if r["memory"]["id"] == pers_id)["final_score"]

    assert abs((score_rule / score_pers) - 2.0) < 1e-6

def test_3_4_persona_access_counter_increment(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "User is highly analytical."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    get1 = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(get1)[1]["memory"]["access_count"] == 0

    server.call_tool("retrieve_memory", {"query": "highly analytical"})
    get2 = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(get2)[1]["memory"]["access_count"] == 1

    server.call_tool("retrieve_memory", {"query": "highly analytical"})
    get3 = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(get3)[1]["memory"]["access_count"] == 2

def test_3_5_multi_persona_cluster_search(server):
    server.call_tool("store_memory", {"layer": "Persona", "content": "User prefers Python scripting."})
    server.call_tool("store_memory", {"layer": "Persona", "content": "User is a senior developer."})

    ret = server.call_tool("retrieve_memory", {"query": "Python developer"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) >= 2
    contents = [r["memory"]["content"] for r in results]
    assert any("Python" in c for c in contents)
    assert any("senior" in c for c in contents)

# ==========================================
# SUITE 4: Experience Reinforcement (5 cases)
# ==========================================

def test_4_1_experience_score_multiplier_success(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Successfully deployed server on AWS."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    get1 = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert parse_tool_result(get1)[1]["memory"]["evaluation_score"] == 1.0

    eval_res = server.call_tool("evaluate_experience", {"memory_id": mem_id, "success": True})
    assert parse_tool_result(eval_res)[1]["status"] == "evaluated"

    get2 = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert abs(parse_tool_result(get2)[1]["memory"]["evaluation_score"] - 1.1) < 1e-9

def test_4_2_experience_score_multiplier_failure(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Experienced database timeout during load."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    eval_res = server.call_tool("evaluate_experience", {"memory_id": mem_id, "success": False})
    assert parse_tool_result(eval_res)[1]["status"] == "evaluated"

    get = server.call_tool("get_memory_by_id", {"id": mem_id})
    assert abs(parse_tool_result(get)[1]["memory"]["evaluation_score"] - 0.8) < 1e-9

def test_4_3_experience_clamping_boundaries(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Exp A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Exp B"}))[1]["memory_id"]

    for _ in range(25):
        server.call_tool("evaluate_experience", {"memory_id": id_a, "success": True})
    for _ in range(25):
        server.call_tool("evaluate_experience", {"memory_id": id_b, "success": False})

    get_a = server.call_tool("get_memory_by_id", {"id": id_a})
    get_b = server.call_tool("get_memory_by_id", {"id": id_b})

    assert parse_tool_result(get_a)[1]["memory"]["evaluation_score"] == 5.0
    assert parse_tool_result(get_b)[1]["memory"]["evaluation_score"] == 0.1

def test_4_4_exemption_from_non_experience_layer_evaluations(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Rules of logic."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    eval_res = server.call_tool("evaluate_experience", {"memory_id": mem_id, "success": True})
    success, data = parse_tool_result(eval_res)
    assert not success
    assert "Memory is not in the Experience layer" in data

def test_4_5_evaluated_experience_ranking_shift(server):
    res_a = server.call_tool("store_memory", {"layer": "Experience", "content": "Method of database backup was fast."})
    res_b = server.call_tool("store_memory", {"layer": "Experience", "content": "Method of database backup was secure."})
    id_a = parse_tool_result(res_a)[1]["memory_id"]
    id_b = parse_tool_result(res_b)[1]["memory_id"]

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_a,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_b,))

    ret1 = server.call_tool("retrieve_memory", {"query": "database backup"})
    results1 = parse_tool_result(ret1)[1]["results"]
    assert len(results1) >= 2
    
    first_id = results1[0]["memory"]["id"]
    second_id = results1[1]["memory"]["id"]

    for _ in range(5):
        server.call_tool("evaluate_experience", {"memory_id": second_id, "success": True})

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_a,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_b,))

    ret2 = server.call_tool("retrieve_memory", {"query": "database backup"})
    results2 = parse_tool_result(ret2)[1]["results"]
    
    assert results2[0]["memory"]["id"] == second_id
    assert results2[1]["memory"]["id"] == first_id

# ==========================================
# SUITE 5: Graph Associations (5 cases)
# ==========================================

def test_5_1_create_association_validates_node_existence(server):
    res = server.call_tool("create_association", {
        "source_id": "does-not-exist-1",
        "target_id": "does-not-exist-2",
        "relation_type": "related_to"
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Source memory with ID" in data and "not found" in data

def test_5_2_self_referential_association_prevention(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Self check memory."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    res_assoc = server.call_tool("create_association", {
        "source_id": mem_id,
        "target_id": mem_id,
        "relation_type": "depends_on"
    })
    success, data = parse_tool_result(res_assoc)
    assert not success
    assert "Self-referential associations are not allowed" in data

def test_5_3_bidirectional_association_direction_queries(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory B"}))[1]["memory_id"]

    server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "depends_on"})

    # Outgoing from A
    out_a = server.call_tool("get_associations", {"source_id": id_a, "direction": "outgoing"})
    assocs_out = parse_tool_result(out_a)[1]["associations"]
    assert len(assocs_out) == 1
    assert assocs_out[0]["source_id"] == id_a and assocs_out[0]["target_id"] == id_b

    # Incoming to B
    in_b = server.call_tool("get_associations", {"source_id": id_b, "direction": "incoming"})
    assocs_in = parse_tool_result(in_b)[1]["associations"]
    assert len(assocs_in) == 1
    assert assocs_in[0]["source_id"] == id_a and assocs_in[0]["target_id"] == id_b

    # Both for A
    both_a = server.call_tool("get_associations", {"source_id": id_a, "direction": "both"})
    assocs_both = parse_tool_result(both_a)[1]["associations"]
    assert len(assocs_both) == 1
    assert assocs_both[0]["source_id"] == id_a and assocs_both[0]["target_id"] == id_b

def test_5_4_whitespace_and_case_validation_in_relation_type(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory B"}))[1]["memory_id"]

    # Whitespace-only
    res_err = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "   "})
    assert not parse_tool_result(res_err)[0]
    assert "Relation type cannot be empty or whitespace-only" in parse_tool_result(res_err)[1]

    # Trimming test
    res_succ = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "   related_to \n  "})
    assert parse_tool_result(res_succ)[0]

    # Verify trimmed
    get_a = server.call_tool("get_associations", {"source_id": id_a, "direction": "outgoing"})
    assoc = parse_tool_result(get_a)[1]["associations"][0]
    assert assoc["relation_type"] == "related_to"

def test_5_5_cascading_delete_integrity(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Memory B"}))[1]["memory_id"]

    server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "relates_to"})
    assert len(parse_tool_result(server.call_tool("get_associations", {"source_id": id_a}))[1]["associations"]) == 1

    server.direct_db_update("DELETE FROM memories WHERE id = ?", (id_b,))
    assert len(parse_tool_result(server.call_tool("get_associations", {"source_id": id_a}))[1]["associations"]) == 0

# ==========================================
# SUITE 6: Vector Search (5 cases)
# ==========================================

def test_6_1_semantic_synonym_matching(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "The programmer favors using Python for backend APIs."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "developer backend language selection choice"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == mem_id

def test_6_2_hybrid_scoring_blend_verification(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "rust compiler system error"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "programming compiler system error"}))[1]["memory_id"]

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_a,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (id_b,))

    ret = server.call_tool("retrieve_memory", {"query": "rust compiler system error"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) >= 2
    assert results[0]["memory"]["id"] == id_a
    assert results[1]["memory"]["id"] == id_b

def test_6_3_handling_empty_query_strings(server):
    ret = server.call_tool("retrieve_memory", {"query": "   "})
    assert parse_tool_result(ret)[1]["results"] == []

def test_6_4_fallback_behaviour_on_embedding_failures(tmp_path):
    db_dir = tmp_path / ".mcp_memory_storage"
    db_dir.mkdir()
    cache_file = db_dir / "models_cache"
    cache_file.write_text("blocked")

    srv = ServerInstance(str(tmp_path), fastembed_cache_path=str(cache_file))
    try:
        res = srv.call_tool("store_memory", {
            "layer": "Rule",
            "content": "The quick brown fox jumps over the lazy dog."
        })
        success, data = parse_tool_result(res)
        assert success
        mem_id = data["memory_id"]

        res_db = srv.direct_db_query("SELECT embedding FROM memories WHERE id = ?", (mem_id,))
        assert len(res_db) == 1
        assert len(res_db[0][0]) == 0

        ret = srv.call_tool("retrieve_memory", {"query": "brown fox"})
        results = parse_tool_result(ret)[1]["results"]
        assert len(results) == 1
        assert results[0]["memory"]["id"] == mem_id
    finally:
        srv.stop()

def test_6_5_multi_vector_cosine_clamp_limits(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Cosine similarity limits check."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    res_db = server.direct_db_query("SELECT embedding FROM memories WHERE id = ?", (mem_id,))
    emb_bytes = res_db[0][0]
    dims = len(emb_bytes) // 4
    assert dims > 0

    nan_float = float('nan')
    nan_bytes = struct.pack(f'<{dims}f', *[nan_float]*dims)

    server.direct_db_update("UPDATE memories SET embedding = ? WHERE id = ?", (sqlite3.Binary(nan_bytes), mem_id))

    ret = server.call_tool("retrieve_memory", {"query": "limits check"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1

# ==========================================
# SUITE 7: Hierarchical Decay (5 cases)
# ==========================================

def test_7_1_multi_layer_comparative_decay_rate(server):
    content = "Multi-layer comparative decay rate target content."
    
    id_rule = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": content}))[1]["memory_id"]
    id_pers = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": content}))[1]["memory_id"]
    id_exp = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": content}))[1]["memory_id"]
    id_sess = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "s1", "content": content}))[1]["memory_id"]

    ret_base = server.call_tool("retrieve_memory", {"query": "comparative decay rate", "session_id": "s1"})
    results_base = parse_tool_result(ret_base)[1]["results"]
    
    base_rule = next(r for r in results_base if r["memory"]["id"] == id_rule)["final_score"]
    base_pers = next(r for r in results_base if r["memory"]["id"] == id_pers)["final_score"]
    base_exp = next(r for r in results_base if r["memory"]["id"] == id_exp)["final_score"]
    base_sess = next(r for r in results_base if r["memory"]["id"] == id_sess)["final_score"]

    target_time = (datetime.utcnow() - timedelta(days=5)).isoformat() + "Z"
    for mid in [id_rule, id_pers, id_exp, id_sess]:
        server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, mid))

    ret_decay = server.call_tool("retrieve_memory", {"query": "comparative decay rate", "session_id": "s1"})
    results_decay = parse_tool_result(ret_decay)[1]["results"]
    
    decay_rule = next(r for r in results_decay if r["memory"]["id"] == id_rule)["final_score"]
    decay_pers = next(r for r in results_decay if r["memory"]["id"] == id_pers)["final_score"]
    decay_exp = next(r for r in results_decay if r["memory"]["id"] == id_exp)["final_score"]
    decay_sess = next(r for r in results_decay if r["memory"]["id"] == id_sess)["final_score"]

    ratio_rule = decay_rule / base_rule
    ratio_pers = decay_pers / base_pers
    ratio_exp = decay_exp / base_exp
    ratio_sess = decay_sess / base_sess

    assert abs(ratio_rule - 1.0) < 0.01
    assert abs(ratio_pers - 0.995) < 0.05
    assert abs(ratio_exp - 0.778) < 0.05
    assert abs(ratio_sess - 0.367) < 0.05

def test_7_2_session_memory_decay_filter_timeout(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "s1",
        "content": "We need to document the database design details."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    ret1 = server.call_tool("retrieve_memory", {"query": "database design details", "session_id": "s1"})
    assert len(parse_tool_result(ret1)[1]["results"]) == 1

    target_time = (datetime.utcnow() - timedelta(days=10)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, mem_id))

    # With session ID, it decays below 0.15 final score
    ret_with_sess = server.call_tool("retrieve_memory", {"query": "database design details", "session_id": "s1"})
    results = parse_tool_result(ret_with_sess)[1]["results"]
    assert len(results) == 1
    assert results[0]["final_score"] < 0.15

    # Without session ID, it is isolated (filtered out completely)
    ret_no_sess = server.call_tool("retrieve_memory", {"query": "database design details"})
    assert len(parse_tool_result(ret_no_sess)[1]["results"]) == 0

def test_7_3_multiplicative_frequency_boost_recovery(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "s1",
        "content": "Design details and implementation plans."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    target_time = (datetime.utcnow() - timedelta(days=5)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, mem_id))

    ret1 = server.call_tool("retrieve_memory", {"query": "Design details", "session_id": "s1"})
    score1 = parse_tool_result(ret1)[1]["results"][0]["final_score"]

    server.direct_db_update("UPDATE memories SET last_accessed = ? WHERE id = ?", (target_time, mem_id))
    ret2 = server.call_tool("retrieve_memory", {"query": "Design details", "session_id": "s1"})
    score2 = parse_tool_result(ret2)[1]["results"][0]["final_score"]

    assert score2 > score1
    assert abs((score2 / score1) - 1.069) < 0.01

def test_7_4_retrievals_update_accessed_timestamp(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Retrieve time check."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    res_db1 = server.direct_db_query("SELECT last_accessed FROM memories WHERE id = ?", (mem_id,))
    t1 = parse_iso_datetime(res_db1[0][0])

    time.sleep(2)

    server.call_tool("retrieve_memory", {"query": "Retrieve time check"})

    res_db2 = server.direct_db_query("SELECT last_accessed FROM memories WHERE id = ?", (mem_id,))
    t2 = parse_iso_datetime(res_db2[0][0])

    assert (t2 - t1).total_seconds() >= 1.5

def test_7_5_retrieval_return_limit_top_5(server):
    mem_ids = []
    for i in range(7):
        res = server.call_tool("store_memory", {
            "layer": "Session",
            "session_id": "session-1",
            "content": f"Common prefix keyword match number {i}."
        })
        mem_ids.append(parse_tool_result(res)[1]["memory_id"])

    ret = server.call_tool("retrieve_memory", {
        "query": "Common prefix keyword match",
        "session_id": "session-1"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 5
    returned_ids = {r["memory"]["id"] for r in results}

    for mid in mem_ids:
        res_db = server.direct_db_query("SELECT access_count FROM memories WHERE id = ?", (mid,))
        count = res_db[0][0]
        if mid in returned_ids:
            assert count == 1
        else:
            assert count == 0

# ==========================================
# TIER 2: Boundary & Corner Cases (35 cases)
# ==========================================

# --- Suite 8: Session Isolation ---

def test_8_1_session_isolation_empty_whitespace_id(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "",
        "content": "Empty session content"
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Session ID is required" in data

    res2 = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "   ",
        "content": "Whitespace session content"
    })
    success2, data2 = parse_tool_result(res2)
    assert not success2
    assert "Session ID is required" in data2

def test_8_2_session_isolation_extremely_long_id(server):
    long_id_a = "session_a_" + "x" * 5000
    long_id_b = "session_b_" + "y" * 5000
    
    res_a = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": long_id_a,
        "content": "Secret code for session A is 999."
    })
    assert parse_tool_result(res_a)[0]

    res_b = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": long_id_b,
        "content": "Secret code for session B is 888."
    })
    assert parse_tool_result(res_b)[0]

    ret_a = server.call_tool("retrieve_memory", {
        "query": "Secret code",
        "session_id": long_id_a
    })
    results_a = parse_tool_result(ret_a)[1]["results"]
    assert len(results_a) == 1
    assert results_a[0]["memory"]["content"] == "Secret code for session A is 999."

    ret_b = server.call_tool("retrieve_memory", {
        "query": "Secret code",
        "session_id": long_id_b
    })
    results_b = parse_tool_result(ret_b)[1]["results"]
    assert len(results_b) == 1
    assert results_b[0]["memory"]["content"] == "Secret code for session B is 888."

def test_8_3_session_isolation_special_characters(server):
    special_id = "sess-!@#$%^&*()_+=-{}[]|\\:;\"'<>,.?/~`"
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": special_id,
        "content": "User prefers tab indentation."
    })
    assert parse_tool_result(res)[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "prefers tab",
        "session_id": special_id
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["content"] == "User prefers tab indentation."

def test_8_4_session_isolation_sql_injection(server):
    sqli_id = "' OR '1'='1"
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": sqli_id,
        "content": "SQL injection session data."
    })
    assert parse_tool_result(res)[0]

    res_norm = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "normal-session",
        "content": "Normal session data."
    })
    assert parse_tool_result(res_norm)[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "session data",
        "session_id": sqli_id
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["content"] == "SQL injection session data."

def test_8_5_session_isolation_non_existent_id(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-1",
        "content": "Important session info."
    })
    assert parse_tool_result(res)[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "Important session info",
        "session_id": "session-nonexistent"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 0

# --- Suite 9: Rule Permanence ---

def test_9_1_rule_permanence_empty_content(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": ""
    })
    success, data = parse_tool_result(res)
    assert success
    rule_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "some search query"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == rule_id
    assert results[0]["memory"]["content"] == ""

def test_9_2_rule_permanence_extremely_long_content(server):
    long_content = "Rule prefix: " + "A" * 40000 + " Rule suffix."
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": long_content
    })
    success, data = parse_tool_result(res)
    assert success
    rule_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "Rule suffix"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == rule_id

def test_9_3_rule_permanence_duplicate_content(server):
    content = "Always format output as JSON."
    res1 = server.call_tool("store_memory", {"layer": "Rule", "content": content})
    res2 = server.call_tool("store_memory", {"layer": "Rule", "content": content})
    assert parse_tool_result(res1)[0]
    assert parse_tool_result(res2)[0]

    ret = server.call_tool("retrieve_memory", {"query": "format output JSON"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 2
    assert results[0]["memory"]["content"] == content
    assert results[1]["memory"]["content"] == content

def test_9_4_rule_permanence_special_payloads(server):
    special_content = "Rule: SELECT * FROM users WHERE 'name' = \"admin\" AND data = '{\"role\": \"user\"}';"
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": special_content
    })
    success, data = parse_tool_result(res)
    assert success
    rule_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "SELECT FROM users"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == rule_id
    assert results[0]["memory"]["content"] == special_content

def test_9_5_rule_permanence_exact_vs_substring(server):
    content = "Strict security policy: encrypt all passwords."
    res = server.call_tool("store_memory", {"layer": "Rule", "content": content})
    rule_id = parse_tool_result(res)[1]["memory_id"]

    ret_exact = server.call_tool("retrieve_memory", {"query": "Strict security policy: encrypt all passwords."})
    results_exact = parse_tool_result(ret_exact)[1]["results"]
    assert len(results_exact) == 1
    assert results_exact[0]["memory"]["id"] == rule_id

    ret_sub = server.call_tool("retrieve_memory", {"query": "encrypt passwords"})
    results_sub = parse_tool_result(ret_sub)[1]["results"]
    assert len(results_sub) == 1
    assert results_sub[0]["memory"]["id"] == rule_id

    ret_sem = server.call_tool("retrieve_memory", {"query": "safety guideline cipher credentials"})
    results_sem = parse_tool_result(ret_sem)[1]["results"]
    assert len(results_sem) == 1
    assert results_sem[0]["memory"]["id"] == rule_id

# --- Suite 10: Persona Adaptation ---

def test_10_1_persona_empty_content(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": ""
    })
    success, data = parse_tool_result(res)
    assert success
    persona_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "developer coding preference"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 0

def test_10_2_persona_extremely_long_content(server):
    long_content = "Persona: " + "P" * 40000 + " final details."
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": long_content
    })
    success, data = parse_tool_result(res)
    assert success
    persona_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "final details"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == persona_id

def test_10_3_persona_retrieve_below_relevance(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "User prefers dark mode UI for all code editors."
    })
    assert parse_tool_result(res)[0]

    ret = server.call_tool("retrieve_memory", {
        "query": "baking apple pie recipe"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 0

def test_10_4_persona_high_frequency_boost(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "User works at a financial startup."
    })
    persona_id = parse_tool_result(res)[1]["memory_id"]

    ret1 = server.call_tool("retrieve_memory", {"query": "financial startup"})
    score1 = parse_tool_result(ret1)[1]["results"][0]["final_score"]

    for _ in range(5):
        server.call_tool("retrieve_memory", {"query": "financial startup"})

    ret2 = server.call_tool("retrieve_memory", {"query": "financial startup"})
    score2 = parse_tool_result(ret2)[1]["results"][0]["final_score"]
    
    assert score2 > score1

def test_10_5_persona_decay_simulated_infinity(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "User likes to write Haskell code."
    })
    persona_id = parse_tool_result(res)[1]["memory_id"]

    target_time = (datetime.utcnow() - timedelta(days=20000)).isoformat() + "Z"
    server.direct_db_update(
        "UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?",
        (target_time, persona_id)
    )

    ret = server.call_tool("retrieve_memory", {"query": "write Haskell code"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["final_score"] < 1e-4

# --- Suite 11: Experience Reinforcement ---

def test_11_1_experience_evaluate_invalid_id(server):
    res = server.call_tool("evaluate_experience", {
        "memory_id": "non-existent-id-xyz",
        "success": True
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Memory not found" in data

def test_11_2_experience_evaluate_clamp_max(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Successfully implemented database connection pooling."
    })
    exp_id = parse_tool_result(res)[1]["memory_id"]

    for _ in range(30):
        server.call_tool("evaluate_experience", {"memory_id": exp_id, "success": True})

    get_res = server.call_tool("get_memory_by_id", {"id": exp_id})
    assert parse_tool_result(get_res)[1]["memory"]["evaluation_score"] == 5.0

def test_11_3_experience_evaluate_clamp_min(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Failed to compile project due to lifetime errors."
    })
    exp_id = parse_tool_result(res)[1]["memory_id"]

    for _ in range(40):
        server.call_tool("evaluate_experience", {"memory_id": exp_id, "success": False})

    get_res = server.call_tool("get_memory_by_id", {"id": exp_id})
    assert parse_tool_result(get_res)[1]["memory"]["evaluation_score"] == 0.1

def test_11_4_experience_evaluate_invalid_params(server):
    req_id = server.id_counter
    server.id_counter += 1
    request = {
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "tools/call",
        "params": {
            "name": "evaluate_experience",
            "arguments": {
                "memory_id": 12345,
                "success": "not-a-bool"
            }
        }
    }
    req_str = json.dumps(request) + "\n"
    server.process.stdin.write(req_str.encode('utf-8'))
    server.process.stdin.flush()

    line = server.process.stdout.readline()
    resp = json.loads(line.decode('utf-8'))
    assert resp.get("jsonrpc") == "2.0"
    if "error" in resp:
        assert resp["error"]["code"] in [-32602, -32603]
    else:
        assert resp["result"]["isError"]

def test_11_5_experience_evaluate_deleted_experience(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Temporary experience memory."
    })
    exp_id = parse_tool_result(res)[1]["memory_id"]

    server.direct_db_update("DELETE FROM memories WHERE id = ?", (exp_id,))

    eval_res = server.call_tool("evaluate_experience", {
        "memory_id": exp_id,
        "success": True
    })
    success, data = parse_tool_result(eval_res)
    assert not success
    assert "Memory not found" in data

# --- Suite 12: Graph Associations ---

def test_12_1_graph_assoc_extreme_relation_length(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node B"}))[1]["memory_id"]

    long_relation = "R" * 5000
    res = server.call_tool("create_association", {
        "source_id": id_a,
        "target_id": id_b,
        "relation_type": long_relation
    })
    assert parse_tool_result(res)[0]

    assocs_res = server.call_tool("get_associations", {"source_id": id_a})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert len(assocs) == 1
    assert assocs[0]["relation_type"] == long_relation

def test_12_2_graph_assoc_special_relation_characters(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node B"}))[1]["memory_id"]

    special_relation = "!@#$%^&*()_+=-{}[]|\\:;\"'<>,.?/~`"
    res = server.call_tool("create_association", {
        "source_id": id_a,
        "target_id": id_b,
        "relation_type": special_relation
    })
    assert parse_tool_result(res)[0]

    assocs_res = server.call_tool("get_associations", {"source_id": id_a})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert len(assocs) == 1
    assert assocs[0]["relation_type"] == special_relation

def test_12_3_graph_assoc_duplicate_overwrite(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node B"}))[1]["memory_id"]

    res1 = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "depends_on"})
    assert parse_tool_result(res1)[0]

    time.sleep(1)

    res2 = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "depends_on"})
    assert parse_tool_result(res2)[0]

    assocs_res = server.call_tool("get_associations", {"source_id": id_a})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert len(assocs) == 1

def test_12_4_graph_assoc_retrieve_empty(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Isolated Node"}))[1]["memory_id"]

    assocs_res = server.call_tool("get_associations", {"source_id": id_a})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert assocs == []

def test_12_5_graph_assoc_one_non_existent_node(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Existing Node"}))[1]["memory_id"]

    res = server.call_tool("create_association", {
        "source_id": id_a,
        "target_id": "non-existent-node-id",
        "relation_type": "relates_to"
    })
    success, data = parse_tool_result(res)
    assert not success
    assert "Target memory with ID" in data and "not found" in data

# --- Suite 13: Vector Search ---

def test_13_1_vector_query_extreme_length(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "System security check."}))[1]["memory_id"]

    extreme_query = "check " + "Q" * 10000
    ret = server.call_tool("retrieve_memory", {"query": extreme_query})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == id_a

def test_13_2_vector_retrieve_empty_database(server):
    ret = server.call_tool("retrieve_memory", {"query": "anything"})
    results = parse_tool_result(ret)[1]["results"]
    assert results == []

def test_13_3_vector_store_extreme_content_embedding(server):
    long_content = "word " * 10000 + "unique_keyword"
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": long_content
    })
    success, data = parse_tool_result(res)
    assert success
    mem_id = data["memory_id"]

    ret = server.call_tool("retrieve_memory", {"query": "unique_keyword"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == mem_id

def test_13_4_vector_query_non_ascii_unicode(server):
    unicode_content = "开发人员喜欢使用🚀Rust编程语言！Σ(⊙▽⊙\")."
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": unicode_content
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    ret = server.call_tool("retrieve_memory", {"query": "Rust编程语言"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == mem_id
    assert results[0]["memory"]["content"] == unicode_content

def test_13_5_vector_retrieve_limit_boundary_zero(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "User is a backend engineer."
    })
    assert parse_tool_result(res)[0]

    ret = server.call_tool("retrieve_memory", {"query": "knitting woolen sweaters for cats"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 0

# --- Suite 14: Hierarchical Decay ---

def test_14_1_decay_exact_threshold(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-1",
        "content": "Specific design documentation on database schemas."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    target_time = (datetime.utcnow() - timedelta(days=5)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, mem_id))

    ret = server.call_tool("retrieve_memory", {
        "query": "database schemas",
        "session_id": "session-1"
    })
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["final_score"] > 0.05

def test_14_2_decay_reinforce_high_frequency(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "s1",
        "content": "Frequent task details."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    target_time = (datetime.utcnow() - timedelta(days=6)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (target_time, mem_id))

    ret1 = server.call_tool("retrieve_memory", {"query": "Frequent task", "session_id": "s1"})
    score1 = parse_tool_result(ret1)[1]["results"][0]["final_score"]

    ret2 = server.call_tool("retrieve_memory", {"query": "Frequent task", "session_id": "s1"})
    score2 = parse_tool_result(ret2)[1]["results"][0]["final_score"]

    ret3 = server.call_tool("retrieve_memory", {"query": "Frequent task", "session_id": "s1"})
    score3 = parse_tool_result(ret3)[1]["results"][0]["final_score"]

    assert score2 > score1
    assert score3 > score2

def test_14_3_future_timestamp_decay(server):
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "s1",
        "content": "Future prediction content."
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    future_time = (datetime.utcnow() + timedelta(days=5)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (future_time, mem_id))

    ret = server.call_tool("retrieve_memory", {"query": "Future prediction", "session_id": "s1"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["final_score"] > 0.5

def test_14_4_decay_dense_top_5_limit(server):
    mem_ids = []
    for i in range(10):
        res = server.call_tool("store_memory", {
            "layer": "Session",
            "session_id": "s1",
            "content": f"Decay limit test memory item number {i}."
        })
        mem_ids.append(parse_tool_result(res)[1]["memory_id"])

    for _ in range(3):
        for i in range(5):
            server.call_tool("retrieve_memory", {"query": f"Decay limit test memory item number {i}.", "session_id": "s1"})

    decay_time = (datetime.utcnow() - timedelta(days=8)).isoformat() + "Z"
    for idx, mid in enumerate(mem_ids):
        ac = 10 if idx < 5 else 0
        server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = ? WHERE id = ?", (decay_time, ac, mid))

    ret = server.call_tool("retrieve_memory", {"query": "Decay limit test memory item", "session_id": "s1"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 5
    returned_ids = {r["memory"]["id"] for r in results}
    for i in range(5):
        assert mem_ids[i] in returned_ids

def test_14_5_decay_concurrent_read(server):
    res1 = server.call_tool("store_memory", {"layer": "Rule", "content": "Concurrent rule A"})
    res2 = server.call_tool("store_memory", {"layer": "Rule", "content": "Concurrent rule B"})
    id_a = parse_tool_result(res1)[1]["memory_id"]
    id_b = parse_tool_result(res2)[1]["memory_id"]

    for _ in range(10):
        server.call_tool("retrieve_memory", {"query": "Concurrent rule"})

    get_a = server.call_tool("get_memory_by_id", {"id": id_a})
    get_b = server.call_tool("get_memory_by_id", {"id": id_b})
    
    assert parse_tool_result(get_a)[1]["memory"]["access_count"] > 0
    assert parse_tool_result(get_b)[1]["memory"]["access_count"] > 0


# ==========================================
# TIER 3: Cross-Feature Combinations (7 cases)
# ==========================================

def test_15_1_session_isolation_and_hierarchical_decay(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "session-A", "content": "Data for session A."}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "session-B", "content": "Data for session B."}))[1]["memory_id"]

    decay_time = (datetime.utcnow() - timedelta(days=12)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (decay_time, id_a))

    ret_a = server.call_tool("retrieve_memory", {"query": "Data for session", "session_id": "session-A"})
    results_a = parse_tool_result(ret_a)[1]["results"]
    assert len(results_a) == 1
    assert results_a[0]["memory"]["id"] == id_a
    assert results_a[0]["final_score"] < 0.2

    ret_b = server.call_tool("retrieve_memory", {"query": "Data for session", "session_id": "session-B"})
    results_b = parse_tool_result(ret_b)[1]["results"]
    assert len(results_b) == 1
    assert results_b[0]["memory"]["id"] == id_b
    assert results_b[0]["final_score"] > 0.5

def test_15_2_rule_permanence_and_experience_reinforcement(server):
    rule_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "System security protocols."}))[1]["memory_id"]
    exp_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Flashed custom firmware successfully."}))[1]["memory_id"]

    eval_rule = server.call_tool("evaluate_experience", {"memory_id": rule_id, "success": True})
    success_rule, err_msg = parse_tool_result(eval_rule)
    assert not success_rule
    assert "Memory is not in the Experience layer" in err_msg

    eval_exp = server.call_tool("evaluate_experience", {"memory_id": exp_id, "success": True})
    assert parse_tool_result(eval_exp)[0]

    get_rule = server.call_tool("get_memory_by_id", {"id": rule_id})
    get_exp = server.call_tool("get_memory_by_id", {"id": exp_id})
    assert parse_tool_result(get_rule)[1]["memory"]["evaluation_score"] == 1.0
    assert abs(parse_tool_result(get_exp)[1]["memory"]["evaluation_score"] - 1.1) < 1e-9

def test_15_3_persona_adaptation_and_vector_search(server):
    pers_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": "User prefers utilizing Python for programming tasks."}))[1]["memory_id"]

    ret1 = server.call_tool("retrieve_memory", {"query": "developer preferences for scripting languages"})
    results1 = parse_tool_result(ret1)[1]["results"]
    
    assert len(results1) == 1
    assert results1[0]["memory"]["id"] == pers_id
    score1 = results1[0]["final_score"]

    ret2 = server.call_tool("retrieve_memory", {"query": "developer preferences for scripting languages"})
    results2 = parse_tool_result(ret2)[1]["results"]
    assert len(results2) == 1
    score2 = results2[0]["final_score"]

    assert score2 > score1

def test_15_4_graph_associations_and_hierarchical_decay(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "s1", "content": "Design doc for API."}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "s1", "content": "Implementation of API."}))[1]["memory_id"]

    server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "leads_to"})

    decay_time = (datetime.utcnow() - timedelta(days=10)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (decay_time, id_b))

    assocs_res = server.call_tool("get_associations", {"source_id": id_a})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert len(assocs) == 1
    assert assocs[0]["target_id"] == id_b

    ret = server.call_tool("retrieve_memory", {"query": "Implementation of API", "session_id": "s1"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) >= 1
    exp_res = next((r for r in results if r["memory"]["id"] == id_b), None)
    assert exp_res is not None
    assert exp_res["final_score"] < 0.15

def test_15_5_session_isolation_and_graph_associations(server):
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "sess-A", "content": "Component A definition."}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": "sess-B", "content": "Component B definition."}))[1]["memory_id"]

    # Prohibited cross-session association
    assoc_res = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "depends_on"})
    assert not parse_tool_result(assoc_res)[0]

    ret_a = server.call_tool("retrieve_memory", {"query": "Component definition", "session_id": "sess-A"})
    results_a = parse_tool_result(ret_a)[1]["results"]
    assert len(results_a) == 1
    assert results_a[0]["memory"]["id"] == id_a

    ret_b = server.call_tool("retrieve_memory", {"query": "Component definition", "session_id": "sess-B"})
    results_b = parse_tool_result(ret_b)[1]["results"]
    assert len(results_b) == 1
    assert results_b[0]["memory"]["id"] == id_b

def test_15_6_rule_permanence_and_vector_search(server):
    rule_id = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "The system must operate in high-availability mode."}))[1]["memory_id"]

    ret = server.call_tool("retrieve_memory", {"query": "always run with redundant nodes"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == rule_id
    score1 = results[0]["final_score"]

    decay_time = (datetime.utcnow() - timedelta(days=100)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (decay_time, rule_id))

    ret2 = server.call_tool("retrieve_memory", {"query": "always run with redundant nodes"})
    results2 = parse_tool_result(ret2)[1]["results"]
    assert len(results2) == 1
    score2 = results2[0]["final_score"]

    assert abs(score1 - score2) < 1e-9

def test_15_7_experience_reinforcement_and_hierarchical_decay(server):
    exp_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Deploying API to dev environment was successful."}))[1]["memory_id"]
    exp_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Deploying API to staging environment was successful."}))[1]["memory_id"]

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (exp_a,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (exp_b,))

    decay_time = (datetime.utcnow() - timedelta(days=5)).isoformat() + "Z"
    server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (decay_time, exp_a))

    ret1 = server.call_tool("retrieve_memory", {"query": "Deploying API"})
    results1 = parse_tool_result(ret1)[1]["results"]
    assert results1[0]["memory"]["id"] == exp_b
    assert results1[1]["memory"]["id"] == exp_a

    for _ in range(5):
        server.call_tool("evaluate_experience", {"memory_id": exp_a, "success": True})

    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (exp_a,))
    server.direct_db_update("UPDATE memories SET access_count = 0 WHERE id = ?", (exp_b,))

    ret2 = server.call_tool("retrieve_memory", {"query": "Deploying API"})
    results2 = parse_tool_result(ret2)[1]["results"]
    assert results2[0]["memory"]["id"] == exp_a
    assert results2[1]["memory"]["id"] == exp_b


# ==========================================
# TIER 4: Real-World Application Scenarios (5 workloads)
# ==========================================

def test_16_1_workload_developer_assistant(server):
    rule_concise = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Rule", "content": "Coding rule: write concise Rust code without nested matches."
    }))[1]["memory_id"]

    pers_rust = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Persona", "content": "Developer prefers async/await with tokio and axum for API development."
    }))[1]["memory_id"]

    sess_id = "dev-session-456"
    res_sess = server.call_tool("store_memory", {
        "layer": "Session", "session_id": sess_id, "content": "Implementing connection pooling error handling today."
    })
    sess_mem_id = parse_tool_result(res_sess)[1]["memory_id"]

    ret = server.call_tool("retrieve_memory", {
        "query": "how to write API endpoint and handle database connection pool error",
        "session_id": sess_id
    })
    results = parse_tool_result(ret)[1]["results"]
    
    assert len(results) >= 3
    assert results[0]["memory"]["layer"] == "Rule"
    assert results[1]["memory"]["layer"] == "Persona"
    assert results[2]["memory"]["layer"] == "Session"

    exp_id = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Experience", "content": "Implemented axum endpoint with SQLx connection pool error fallback."
    }))[1]["memory_id"]

    server.call_tool("evaluate_experience", {"memory_id": exp_id, "success": True})

    server.call_tool("create_association", {
        "source_id": sess_mem_id, "target_id": exp_id, "relation_type": "resolved_by"
    })

    ret2 = server.call_tool("retrieve_memory", {
        "query": "SQLx connection pool fallback handler",
        "session_id": sess_id
    })
    results2 = parse_tool_result(ret2)[1]["results"]
    assert results2[0]["memory"]["id"] == exp_id

def test_16_2_workload_persona_driven_recommendation(server):
    pers_scifi = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": "User is a fan of hard sci-fi novels and space exploration."}))[1]["memory_id"]
    pers_audio = parse_tool_result(server.call_tool("store_memory", {"layer": "Persona", "content": "User prefers listening to audiobooks during long commutes."}))[1]["memory_id"]

    exp_dune = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Listened to Dune audiobook on Audible."}))[1]["memory_id"]
    server.call_tool("evaluate_experience", {"memory_id": exp_dune, "success": True})

    exp_cooking = parse_tool_result(server.call_tool("store_memory", {"layer": "Experience", "content": "Read a recipe book about French desserts."}))[1]["memory_id"]
    server.call_tool("evaluate_experience", {"memory_id": exp_cooking, "success": False})

    ret = server.call_tool("retrieve_memory", {"query": "suggest next audiobook or audiobooks reading list for sci-fi fan"})
    results = parse_tool_result(ret)[1]["results"]

    retrieved_ids = {r["memory"]["id"] for r in results}
    assert pers_scifi in retrieved_ids
    assert pers_audio in retrieved_ids
    assert exp_dune in retrieved_ids
    # exp_cooking may have a low semantic score and be retrieved in a small database, but it must not be in the top 3 recommendations
    assert exp_cooking not in {r["memory"]["id"] for r in results[:3]}

def test_16_3_workload_consolidation_decay(server):
    sess_id = "session-consolidation"
    mem1 = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": sess_id, "content": "Discussed migrating database from Postgres to SQLite."}))[1]["memory_id"]
    mem2 = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": sess_id, "content": "Faced issues with SQLite concurrency and WAL mode."}))[1]["memory_id"]
    mem3 = parse_tool_result(server.call_tool("store_memory", {"layer": "Session", "session_id": sess_id, "content": "Resolved SQLite write contention by using connection pooling limit 1."}))[1]["memory_id"]

    decay_time = (datetime.utcnow() - timedelta(days=15)).isoformat() + "Z"
    for mid in [mem1, mem2, mem3]:
        server.direct_db_update("UPDATE memories SET last_accessed = ?, access_count = 0 WHERE id = ?", (decay_time, mid))

    exp_consolidated = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Database migration lessons: SQLite concurrency is solved by using WAL mode and max connections = 1."
    }))[1]["memory_id"]
    server.call_tool("evaluate_experience", {"memory_id": exp_consolidated, "success": True})

    for mid in [mem1, mem2, mem3]:
        server.call_tool("create_association", {"source_id": mid, "target_id": exp_consolidated, "relation_type": "consolidated_into"})

    ret = server.call_tool("retrieve_memory", {
        "query": "SQLite database migration lessons write contention WAL mode",
        "session_id": sess_id
    })
    results = parse_tool_result(ret)[1]["results"]

    assert results[0]["memory"]["id"] == exp_consolidated
    for r in results[1:]:
        assert r["memory"]["layer"] == "Session"
        assert r["final_score"] < 0.15

def test_16_4_workload_error_recovery_robustness(server):
    res_err = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "",
        "content": "This should fail"
    })
    assert not parse_tool_result(res_err)[0]

    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node B"}))[1]["memory_id"]

    res_assoc1 = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "related"})
    res_assoc2 = server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "related"})
    assert parse_tool_result(res_assoc1)[0]
    assert parse_tool_result(res_assoc2)[0]

    res_self = server.call_tool("create_association", {"source_id": id_a, "target_id": id_a, "relation_type": "depends"})
    assert not parse_tool_result(res_self)[0]

    server.stop()
    server.start()

    ret = server.call_tool("retrieve_memory", {"query": "Node A"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) >= 1
    assert results[0]["memory"]["id"] == id_a

def test_16_5_workload_multisession_knowledge_graph(server):
    id_py = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Session", "session_id": "sess-python", "content": "Python is a dynamic programming language."
    }))[1]["memory_id"]

    id_rs = parse_tool_result(server.call_tool("store_memory", {
        "layer": "Rule", "content": "Rust is a statically-typed systems language."
    }))[1]["memory_id"]

    res_assoc = server.call_tool("create_association", {
        "source_id": id_py, "target_id": id_rs, "relation_type": "interoperates_via_ffi"
    })
    assert parse_tool_result(res_assoc)[0]

    assocs_res = server.call_tool("get_associations", {"source_id": id_py})
    assocs = parse_tool_result(assocs_res)[1]["associations"]
    assert len(assocs) == 1
    assert assocs[0]["target_id"] == id_rs
    assert assocs[0]["relation_type"] == "interoperates_via_ffi"

    assocs_res_in = server.call_tool("get_associations", {"source_id": id_rs, "direction": "incoming"})
    assocs_in = parse_tool_result(assocs_res_in)[1]["associations"]
    assert len(assocs_in) == 1
    assert assocs_in[0]["source_id"] == id_py
    assert assocs_in[0]["relation_type"] == "interoperates_via_ffi"

    ret_py = server.call_tool("retrieve_memory", {"query": "programming language", "session_id": "sess-python"})
    results_py = parse_tool_result(ret_py)[1]["results"]
    assert len(results_py) >= 1
    assert any(r["memory"]["id"] == id_py for r in results_py)


# ==========================================
# SUITE 17: Adversarial Challenges (6 cases)
# ==========================================

def test_17_1_access_count_overflow_panic(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Rule for overflow testing"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    # Manually update access_count to u32::MAX
    server.direct_db_update(
        "UPDATE memories SET access_count = 4294967295 WHERE id = ?",
        (mem_id,)
    )

    # Calling retrieve should succeed and not crash
    ret = server.call_tool("retrieve_memory", {"query": "overflow testing"})
    success, data = parse_tool_result(ret)
    assert success
    assert len(data["results"]) > 0


def test_17_2_extreme_last_accessed_timestamps(server):
    res = server.call_tool("store_memory", {
        "layer": "Persona",
        "content": "Extreme timestamp testing"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    # Extreme past: year 1000
    server.direct_db_update(
        "UPDATE memories SET last_accessed = '1000-01-01T00:00:00Z' WHERE id = ?",
        (mem_id,)
    )
    ret_past = server.call_tool("retrieve_memory", {"query": "Extreme timestamp"})
    results_past = parse_tool_result(ret_past)[1]["results"]
    assert len(results_past) == 1
    assert results_past[0]["final_score"] < 1e-9

    # Extreme future: year 9999
    server.direct_db_update(
        "UPDATE memories SET last_accessed = '9999-12-31T23:59:59Z' WHERE id = ?",
        (mem_id,)
    )
    ret_future = server.call_tool("retrieve_memory", {"query": "Extreme timestamp"})
    results_future = parse_tool_result(ret_future)[1]["results"]
    assert len(results_future) == 1
    assert results_future[0]["memory"]["id"] == mem_id


def test_17_3_malformed_datetime_handling(server):
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Malformed datetime rule"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    server.direct_db_update(
        "UPDATE memories SET last_accessed = 'invalid-datetime-string' WHERE id = ?",
        (mem_id,)
    )
    
    # Retrieve should return an error gracefully rather than panicking/crashing
    ret = server.call_tool("retrieve_memory", {"query": "Malformed datetime"})
    success, err_msg = parse_tool_result(ret)
    assert not success
    assert "conversion" in err_msg.lower() or "parsing" in err_msg.lower() or "invalid characters" in err_msg.lower()


def test_17_4_extremely_large_tag_list(server):
    large_tags = [f"tag_{i}" for i in range(5000)]
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Large tag list rule",
        "context_tags": large_tags
    })
    success, data = parse_tool_result(res)
    assert success
    mem_id = data["memory_id"]

    get_res = server.call_tool("get_memory_by_id", {"id": mem_id})
    success_get, memory_data = parse_tool_result(get_res)
    assert success_get
    assert len(memory_data["memory"]["tags"]) == 5000


@pytest.mark.skipif(sys.version_info < (3, 11), reason="Python < 3.11 sqlite3 binds float('nan') as NULL")
def test_17_5_nan_evaluation_score_behavior(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "NaN score testing"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    # Update evaluation_score to NaN
    server.direct_db_update(
        "UPDATE memories SET evaluation_score = ? WHERE id = ?",
        (float('nan'), mem_id)
    )

    ret = server.call_tool("retrieve_memory", {"query": "NaN score"})
    results = parse_tool_result(ret)[1]["results"]
    assert len(results) == 1
    assert results[0]["memory"]["id"] == mem_id
    assert results[0]["final_score"] is None


def test_17_6_sqlite_evaluation_score_negative_constraint(server):
    res = server.call_tool("store_memory", {
        "layer": "Experience",
        "content": "Negative score constraint testing"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    with pytest.raises(sqlite3.IntegrityError) as exc_info:
        server.direct_db_update(
            "UPDATE memories SET evaluation_score = -1.0 WHERE id = ?",
            (mem_id,)
        )
    assert "CHECK constraint failed" in str(exc_info.value)


def test_17_7_get_memory_by_id_session_leakage(server):
    # Store a private session memory in session-A
    res = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-A",
        "content": "Secret content for session A"
    })
    success, data = parse_tool_result(res)
    assert success
    mem_id = data["memory_id"]

    # Now, attempt to fetch this memory by ID without any session ID context (global request)
    get_res = server.call_tool("get_memory_by_id", {"id": mem_id})
    success_get, error_msg = parse_tool_result(get_res)
    assert not success_get
    assert "not found" in error_msg.lower()

    # Attempt to fetch with incorrect session ID context
    get_res_wrong = server.call_tool("get_memory_by_id", {"id": mem_id, "session_id": "session-B"})
    success_get_wrong, error_msg_wrong = parse_tool_result(get_res_wrong)
    assert not success_get_wrong
    assert "not found" in error_msg_wrong.lower()

    # Attempt to fetch with correct session ID context
    get_res_correct = server.call_tool("get_memory_by_id", {"id": mem_id, "session_id": "session-A"})
    success_get_correct, memory_data = parse_tool_result(get_res_correct)
    assert success_get_correct
    assert memory_data["memory"]["content"] == "Secret content for session A"
    assert memory_data["memory"]["session_id"] == "session-A"


def test_17_8_cross_session_association_leakage(server):
    # Store private session memory in session-A
    res_a = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-A",
        "content": "Component A in session A"
    })
    id_a = parse_tool_result(res_a)[1]["memory_id"]

    # Store private session memory in session-B
    res_b = server.call_tool("store_memory", {
        "layer": "Session",
        "session_id": "session-B",
        "content": "Component B in session B"
    })
    id_b = parse_tool_result(res_b)[1]["memory_id"]

    # Create an association between them (cross-session!) - should fail
    assoc_res = server.call_tool("create_association", {
        "source_id": id_a,
        "target_id": id_b,
        "relation_type": "cross_link"
    })
    success_assoc, err_msg = parse_tool_result(assoc_res)
    assert not success_assoc
    assert "Cross-session association is prohibited" in err_msg


def test_17_9_invalid_sqlite_types_error_recovery(server):
    # Store a memory
    res = server.call_tool("store_memory", {
        "layer": "Rule",
        "content": "Type corruption testing"
    })
    mem_id = parse_tool_result(res)[1]["memory_id"]

    # Corrupt the access_count by updating it to a text value in SQLite
    server.direct_db_update(
        "UPDATE memories SET access_count = 'corrupted_text' WHERE id = ?",
        (mem_id,)
    )

    # Retrieval should return an error gracefully rather than crashing the server
    ret = server.call_tool("retrieve_memory", {"query": "Type corruption"})
    success, err_msg = parse_tool_result(ret)
    assert not success
    assert "invalid column type" in err_msg.lower() or "invalid type" in err_msg.lower() or "fromsqlconversionfailure" in err_msg.lower()


def test_17_10_sqlite_concurrency_busy_timeout(tmp_path):
    # Use a shared directory for database
    shared_dir = str(tmp_path)
    
    server1 = ServerInstance(shared_dir)
    server2 = ServerInstance(shared_dir)
    
    try:
        # Store memories in both servers (this is sequential, which is fine)
        res1 = server1.call_tool("store_memory", {
            "layer": "Rule",
            "content": "Common rule for concurrent retrieval"
        })
        assert parse_tool_result(res1)[0]
        
        # Now, try to perform concurrent retrievals on both servers.
        # Since retrieve writes to the database (updates access_count and last_accessed),
        # they will attempt to lock the database concurrently.
        # Because no busy timeout is set, they might fail immediately.
        import concurrent.futures
        
        def run_retrieve(srv):
            return srv.call_tool("retrieve_memory", {"query": "concurrent retrieval"})
            
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            # We run 20 concurrent queries (10 on each server)
            futures = []
            for _ in range(10):
                futures.append(executor.submit(run_retrieve, server1))
                futures.append(executor.submit(run_retrieve, server2))
                
            results = [f.result() for f in futures]
            
            # Check if any request returned an error or isError
            errors = [r for r in results if "isError" in r and r["isError"]]
            print(f"Concurrent retrieval errors: {len(errors)}")
            
    finally:
        server1.stop()
        server2.stop()


def test_17_11_circular_graph_cascade_delete(server):
    # Store three memories A, B, C
    id_a = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node A"}))[1]["memory_id"]
    id_b = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node B"}))[1]["memory_id"]
    id_c = parse_tool_result(server.call_tool("store_memory", {"layer": "Rule", "content": "Node C"}))[1]["memory_id"]

    # Create circular associations A -> B -> C -> A
    assert parse_tool_result(server.call_tool("create_association", {"source_id": id_a, "target_id": id_b, "relation_type": "links_to"}))[0]
    assert parse_tool_result(server.call_tool("create_association", {"source_id": id_b, "target_id": id_c, "relation_type": "links_to"}))[0]
    assert parse_tool_result(server.call_tool("create_association", {"source_id": id_c, "target_id": id_a, "relation_type": "links_to"}))[0]

    # Verify associations are all queryable
    assocs_a = parse_tool_result(server.call_tool("get_associations", {"source_id": id_a}))[1]["associations"]
    assert len(assocs_a) == 1
    assert assocs_a[0]["target_id"] == id_b

    # Now delete node A directly from database
    server.direct_db_update("DELETE FROM memories WHERE id = ?", (id_a,))

    # Verify that the association A -> B and C -> A are cascade-deleted, and only B -> C remains
    # Since A is deleted, get_associations for A should fail with not found
    get_a_res = server.call_tool("get_associations", {"source_id": id_a})
    assert not parse_tool_result(get_a_res)[0]

    # Get associations for B (should still have B -> C)
    assocs_b = parse_tool_result(server.call_tool("get_associations", {"source_id": id_b}))[1]["associations"]
    assert len(assocs_b) == 1
    assert assocs_b[0]["target_id"] == id_c

    # Check associations targeting C (incoming) - C -> A was targeting A, so it should be cascade-deleted!
    assocs_c_in = parse_tool_result(server.call_tool("get_associations", {"source_id": id_c, "direction": "incoming"}))[1]["associations"]
    assert len(assocs_c_in) == 1
    assert assocs_c_in[0]["source_id"] == id_b

