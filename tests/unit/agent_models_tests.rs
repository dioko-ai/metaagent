use super::*;

#[test]
fn defaults_use_large_smart_for_all_agent_slots() {
    let routing = CodexAgentModelRouting::default();

    let master = routing.profile_for(CodexAgentKind::Master);
    let auditor = routing.profile_for(CodexAgentKind::WorkerAuditor);
    let docs = routing.profile_for(CodexAgentKind::DocsAttach);

    assert_eq!(master.model, "gpt-5.3-codex");
    assert_eq!(master.thinking_effort.as_deref(), Some("medium"));
    assert_eq!(auditor.model, "gpt-5.3-codex");
    assert_eq!(docs.model, "gpt-5.3-codex");
}

#[test]
fn empty_toml_still_uses_embedded_defaults() {
    let routing = CodexAgentModelRouting::from_toml_str("").expect("parse should succeed");
    let master = routing.profile_for(CodexAgentKind::Master);
    assert_eq!(master.model, "gpt-5.3-codex");
    assert_eq!(master.thinking_effort.as_deref(), Some("medium"));
}

#[test]
fn defaults_include_supergenious_alias_labels() {
    let routing = CodexAgentModelRouting::default();
    let parsed = CodexAgentModelRouting::from_toml_str(
        r#"
        [codex.agent_profiles]
        worker_implementor = "small-supergenious"
        "#,
    )
    .expect("parse should succeed");

    let baseline = routing.profile_for(CodexAgentKind::WorkerImplementor);
    let aliased = parsed.profile_for(CodexAgentKind::WorkerImplementor);
    assert_eq!(aliased.model, "gpt-5.1-codex-mini");
    assert_eq!(aliased.thinking_effort.as_deref(), Some("xhigh"));
    assert_ne!(aliased, baseline);
}

#[test]
fn toml_overrides_profiles_and_agent_assignments() {
    let routing = CodexAgentModelRouting::from_toml_str(
        r#"
        [codex.model_profiles.custom-max]
        model = "gpt-5.3-codex"
        thinking_effort = "xhigh"

        [codex.agent_profiles]
        master = "CUSTOM-max"
        worker_implementor = "custom-max"
        "#,
    )
    .expect("parse should succeed");

    let master = routing.profile_for(CodexAgentKind::Master);
    let implementor = routing.profile_for(CodexAgentKind::WorkerImplementor);
    let task_check = routing.profile_for(CodexAgentKind::TaskCheck);

    assert_eq!(master.model, "gpt-5.3-codex");
    assert_eq!(master.thinking_effort.as_deref(), Some("xhigh"));
    assert_eq!(implementor.thinking_effort.as_deref(), Some("xhigh"));
    assert_eq!(task_check.model, "gpt-5.3-codex");
    assert_eq!(task_check.thinking_effort.as_deref(), Some("medium"));
}

#[test]
fn unknown_profile_assignment_falls_back_to_large_smart() {
    let routing = CodexAgentModelRouting::from_toml_str(
        r#"
        [codex.agent_profiles]
        master = "does-not-exist"
        "#,
    )
    .expect("parse should succeed");

    let master = routing.profile_for(CodexAgentKind::Master);
    assert_eq!(master.model, "gpt-5.3-codex");
    assert_eq!(master.thinking_effort.as_deref(), Some("medium"));
}
