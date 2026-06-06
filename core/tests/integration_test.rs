use anyhow::Result;
use astro_member::memory_manager::MemoryManager;
use astro_member::models::MemoryLayer;

#[test]
fn test_integration_memory_manager_flow() -> Result<()> {
    // 1. Initialize MemoryManager with in-memory database
    let mut manager = MemoryManager::new(":memory:")?;

    // 2. Store memories in different layers
    let rule_id = manager.store(
        MemoryLayer::Rule,
        None,
        "Astro-member must maintain strict integrity rules.".to_string(),
        vec!["integrity".to_string(), "rules".to_string()],
    )?;

    let persona_id = manager.store(
        MemoryLayer::Persona,
        None,
        "Helpful assistant persona with Rust programming expertise.".to_string(),
        vec!["rust".to_string(), "persona".to_string()],
    )?;

    let session_id = manager.store(
        MemoryLayer::Session,
        Some("session-xyz".to_string()),
        "The developer wants a multi-crate cargo workspace.".to_string(),
        vec!["refactoring".to_string()],
    )?;

    // 3. Verify retrieval by query
    let results_rule = manager.retrieve("integrity rules", None)?;
    assert!(!results_rule.is_empty(), "Should retrieve rule memory");
    let has_rule_memory = results_rule.iter().any(|res| res.memory.id == rule_id);
    assert!(has_rule_memory, "Should retrieve the rule memory");

    // 4. Verify session isolation
    let results_session_correct = manager.retrieve("cargo workspace", Some("session-xyz".to_string()))?;
    assert!(!results_session_correct.is_empty(), "Should retrieve session memory");
    let has_session_memory = results_session_correct.iter().any(|res| res.memory.id == session_id);
    assert!(has_session_memory, "Should retrieve the session memory");

    let results_session_incorrect = manager.retrieve("cargo workspace", Some("session-wrong".to_string()))?;
    let has_session_memory_wrong = results_session_incorrect.iter().any(|res| res.memory.id == session_id);
    assert!(!has_session_memory_wrong, "Session isolation should filter out mismatched session ID");

    // 5. Create association
    manager.create_association(&rule_id, &persona_id, "influences")?;
    let assocs = manager.get_associations(&rule_id, Some("outgoing"))?;
    assert_eq!(assocs.len(), 1);
    assert_eq!(assocs[0].target_id, persona_id);
    assert_eq!(assocs[0].relation_type, "influences");

    // 6. Retrieve by ID
    let mem = manager.get_memory_by_id(&rule_id, None)?;
    assert!(mem.is_some());
    assert_eq!(mem.unwrap().content, "Astro-member must maintain strict integrity rules.");

    Ok(())
}

#[test]
fn test_integration_experience_evolution() -> Result<()> {
    let mut manager = MemoryManager::new(":memory:")?;

    let exp_id = manager.store(
        MemoryLayer::Experience,
        None,
        "Refactoring repository structure task".to_string(),
        vec!["refactoring".to_string()],
    )?;

    // Retrieve the initial entry
    let entry = manager.get_memory_by_id(&exp_id, None)?.unwrap();
    assert_eq!(entry.evaluation_score, 1.0);

    // Evaluate success
    manager.evaluate_experience(&exp_id, true)?;
    let entry_after = manager.get_memory_by_id(&exp_id, None)?.unwrap();
    assert!(entry_after.evaluation_score > 1.0, "Score should evolve upwards upon success");

    Ok(())
}
