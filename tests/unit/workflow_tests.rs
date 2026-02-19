use super::*;

fn seed_single_default_task(wf: &mut Workflow, title: &str) {
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: title.to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "implementor details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
    ])
    .expect("seed plan should sync");
}

fn seed_two_default_tasks(wf: &mut Workflow, first_title: &str, second_title: &str) {
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top1".to_string(),
            title: first_title.to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl1".to_string(),
            title: "Implementation".to_string(),
            details: "implementor details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl1-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw1".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top1".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw1-runner".to_string(),
            title: "Run tests".to_string(),
            details: "test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top2".to_string(),
            title: second_title.to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "impl2".to_string(),
            title: "Implementation".to_string(),
            details: "implementor details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top2".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl2-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl2".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw2".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top2".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw2-runner".to_string(),
            title: "Run tests".to_string(),
            details: "test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw2".to_string()),
            order: Some(0),
        },
    ])
    .expect("seed plan should sync");
}

fn seed_single_default_task_with_final_audit(wf: &mut Workflow, title: &str) {
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: title.to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "implementor details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "fa".to_string(),
            title: "Final Audit".to_string(),
            details: "final audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("seed plan should sync");
}

fn complete_non_final_branches_and_start_final_audit(wf: &mut Workflow) -> StartedJob {
    wf.start_execution();

    let implementor = wf.start_next_job().expect("implementor");
    assert_eq!(implementor.role, WorkerRole::Implementor);
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let test_writer = wf.start_next_job().expect("test writer");
    assert_eq!(test_writer.role, WorkerRole::TestWriter);
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner");
    assert_eq!(runner.role, WorkerRole::TestRunner);
    wf.append_active_output("all passed".to_string());
    wf.finish_active_job(true, 0);

    let final_audit = wf.start_next_job().expect("final audit");
    assert_eq!(final_audit.role, WorkerRole::FinalAudit);
    final_audit
}

#[test]
fn syncs_planner_tasks_from_file_entries() {
    let mut wf = Workflow::default();
    let count = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "parent".to_string(),
                title: "Parent".to_string(),
                details: "Parent details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "child".to_string(),
                title: "Child".to_string(),
                details: "Child details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::InProgress,
                parent_id: Some("parent".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "child-audit".to_string(),
                title: "Audit".to_string(),
                details: "Audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("child".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");
    assert_eq!(count, 3);
    let lines = wf.right_pane_lines().join("\n");
    assert!(lines.contains("Task: Parent"));
    assert!(lines.contains("Impl: Child"));
}

#[test]
fn renders_nested_task_blocks_with_local_wrapping() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "p".to_string(),
            title: "Parent task with a long title".to_string(),
            details: "Parent detail text for rendering.".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "c".to_string(),
            title: "Child task title".to_string(),
            details: "Child detail text for rendering.".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("p".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "c-audit".to_string(),
            title: "Child audit".to_string(),
            details: "Audit detail text for rendering.".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("c".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    let lines = wf
        .right_pane_block_view(24, &HashSet::new())
        .lines
        .join("\n");
    assert!(lines.contains("Parent task with a long title"));
    assert!(lines.contains("┌"));
    assert!(lines.contains("└"));
    assert!(lines.contains("  ┌"));
    assert!(lines.contains("  └"));
    assert!(!lines.contains("Task: Parent task with a long title"));
}

#[test]
fn right_pane_block_view_numbers_and_separates_top_level_tasks() {
    let mut wf = Workflow::default();
    seed_two_default_tasks(&mut wf, "Task One", "Task Two");

    let lines = wf.right_pane_block_view(40, &HashSet::new()).lines;
    let first_title = lines
        .iter()
        .position(|line| line == "  1. Task One")
        .expect("first top-level title should render");
    assert!(
        lines
            .get(first_title + 1)
            .is_some_and(|line| line.trim().is_empty()),
        "expected one spacer line between top-level title and details"
    );
    assert!(lines.iter().any(|line| line == "  2. Task Two"));
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("  {}", "─".repeat(38)))
    );
}

#[test]
fn execution_queues_implementor_and_test_writer_jobs() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");

    wf.start_execution();
    let first = wf.start_next_job().expect("first job should start");
    assert_eq!(first.role, WorkerRole::Implementor);
    assert!(matches!(first.run, JobRun::AgentPrompt(_)));
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let second = wf.start_next_job().expect("second job should start");
    assert_eq!(second.role, WorkerRole::Auditor);
    assert!(matches!(second.run, JobRun::AgentPrompt(_)));
}

#[test]
fn top_task_does_not_complete_until_all_implementor_branches_are_done() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Top".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1".to_string(),
            title: "Impl 1".to_string(),
            details: "impl 1 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1-audit".to_string(),
            title: "Impl 1 audit".to_string(),
            details: "impl 1 audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl-1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-2".to_string(),
            title: "Impl 2".to_string(),
            details: "impl 2 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "impl-2-audit".to_string(),
            title: "Impl 2 audit".to_string(),
            details: "impl 2 audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl-2".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();

    let implementor = wf.start_next_job().expect("implementor");
    assert_eq!(implementor.role, WorkerRole::Implementor);
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let snapshot = wf.planner_tasks_for_file();
    let top_status = snapshot
        .iter()
        .find(|entry| entry.id == "top")
        .map(|entry| entry.status);
    let impl_1_status = snapshot
        .iter()
        .find(|entry| entry.id == "impl-1")
        .map(|entry| entry.status);
    let impl_2_status = snapshot
        .iter()
        .find(|entry| entry.id == "impl-2")
        .map(|entry| entry.status);

    assert_ne!(top_status, Some(PlannerTaskStatusFile::Done));
    assert_eq!(impl_1_status, Some(PlannerTaskStatusFile::Done));
    assert_ne!(impl_2_status, Some(PlannerTaskStatusFile::Done));
}

#[test]
fn top_task_does_not_complete_until_all_test_writer_branches_are_done() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Top".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Impl".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Impl audit".to_string(),
            details: "impl audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-1".to_string(),
            title: "Write tests 1".to_string(),
            details: "tw 1 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-1-runner".to_string(),
            title: "Run tests 1".to_string(),
            details: "runner 1 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw-1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-2".to_string(),
            title: "Write tests 2".to_string(),
            details: "tw 2 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(2),
        },
        PlannerTaskFileEntry {
            id: "tw-2-runner".to_string(),
            title: "Run tests 2".to_string(),
            details: "runner 2 details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw-2".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();

    let test_writer = wf.start_next_job().expect("test writer");
    assert_eq!(test_writer.role, WorkerRole::TestWriter);
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let test_runner = wf.start_next_job().expect("test runner");
    assert_eq!(test_runner.role, WorkerRole::TestRunner);
    wf.append_active_output("all passed".to_string());
    wf.finish_active_job(true, 0);

    let snapshot = wf.planner_tasks_for_file();
    let top_status = snapshot
        .iter()
        .find(|entry| entry.id == "top")
        .map(|entry| entry.status);
    let tw_1_status = snapshot
        .iter()
        .find(|entry| entry.id == "tw-1")
        .map(|entry| entry.status);
    let tw_2_status = snapshot
        .iter()
        .find(|entry| entry.id == "tw-2")
        .map(|entry| entry.status);

    assert_ne!(top_status, Some(PlannerTaskStatusFile::Done));
    assert_eq!(tw_1_status, Some(PlannerTaskStatusFile::Done));
    assert_ne!(tw_2_status, Some(PlannerTaskStatusFile::Done));
}

#[test]
fn top_level_tasks_run_sequentially_in_order() {
    let mut wf = Workflow::default();
    seed_two_default_tasks(&mut wf, "Task One", "Task Two");
    wf.start_execution();

    let first = wf.start_next_job().expect("first top-level job");
    let first_top_id = first.top_task_id;
    let mut first_done = false;
    let mut saw_second_top = false;
    let mut next = Some(first);

    for _ in 0..24 {
        let job = next
            .take()
            .unwrap_or_else(|| wf.start_next_job().expect("queued job"));
        if job.top_task_id != first_top_id {
            saw_second_top = true;
            assert!(
                first_done,
                "second top-level task started before the first one completed"
            );
            break;
        }

        match job.role {
            WorkerRole::Implementor => wf.append_active_output("implemented".to_string()),
            WorkerRole::TestWriter => wf.append_active_output("wrote tests".to_string()),
            WorkerRole::Auditor => {
                wf.append_active_output("AUDIT_RESULT: PASS".to_string());
                wf.append_active_output("No issues found".to_string());
            }
            WorkerRole::TestRunner => wf.append_active_output("all passed".to_string()),
            WorkerRole::FinalAudit => wf.append_active_output("No issues found".to_string()),
        }
        let messages = wf.finish_active_job(true, 0);
        if messages
            .iter()
            .any(|m| m.contains(&format!("Task #{} completed", first_top_id)))
        {
            first_done = true;
        }
    }

    assert!(first_done, "first top-level task never completed");
    assert!(
        saw_second_top,
        "second top-level task never started after first completion"
    );
}

#[test]
fn deterministic_test_runner_loops_back_to_test_writer_on_failure() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner");
    assert_eq!(runner.role, WorkerRole::TestRunner);
    assert!(matches!(runner.run, JobRun::DeterministicTestRun));
    wf.append_active_output("test failure output".to_string());
    let messages = wf.finish_active_job(false, 101);
    assert!(messages.iter().any(|m| m.contains("tests failed")));

    let retry = wf.start_next_job().expect("test writer retry");
    assert_eq!(retry.role, WorkerRole::TestWriter);
    match retry.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("Deterministic test run failed"));
            assert!(prompt.contains("test failure output"));
        }
        JobRun::DeterministicTestRun => panic!("expected agent prompt"),
    }
}

#[test]
fn missing_test_command_keeps_required_test_runner_branch_incomplete() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner");
    assert_eq!(runner.role, WorkerRole::TestRunner);
    wf.append_active_output(
        "Deterministic test runner failed: no test command configured in meta.json.".to_string(),
    );
    let messages = wf.finish_active_job(false, -2);
    assert!(messages.iter().any(|m| m.contains("tests failed")));

    let retry = wf.start_next_job().expect("test writer retry");
    assert_eq!(retry.role, WorkerRole::TestWriter);

    let tree = wf.right_pane_lines().join("\n");
    assert!(
        !tree.contains("[x] Task: Do work"),
        "top task should not be marked complete when test command is missing"
    );
}

#[test]
fn test_writer_child_auditor_runs_before_test_runner_when_present() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Do work".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Impl".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Impl audit".to_string(),
            details: "impl audit".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "tw details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-audit".to_string(),
            title: "Audit tests".to_string(),
            details: "audit test details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "run tests".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let writer = wf.start_next_job().expect("test writer");
    assert_eq!(writer.role, WorkerRole::TestWriter);
    wf.append_active_output("Wrote tests".to_string());
    let messages = wf.finish_active_job(true, 0);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test-writer audit queued"))
    );

    let auditor = wf.start_next_job().expect("test-writer auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    match auditor.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("reviewing test-writing output"));
            assert!(prompt.contains("Parent test-writer details:"));
            assert!(prompt.contains("tw details"));
            assert!(prompt.contains("do not run tests"));
            assert!(prompt.contains("do not execute/check shell commands"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner");
    assert_eq!(runner.role, WorkerRole::TestRunner);
}

#[test]
fn failing_test_writer_child_auditor_queues_test_writer_retry() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Do work".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Impl".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Impl audit".to_string(),
            details: "impl audit".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "tw details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-audit".to_string(),
            title: "Audit tests".to_string(),
            details: "audit test details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "run tests".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("Wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test-writer auditor");
    wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
    wf.append_active_output("Missing edge-case assertions".to_string());
    let messages = wf.finish_active_job(true, 0);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("test-writer audit requested fixes"))
    );

    let retry = wf.start_next_job().expect("test writer retry");
    assert_eq!(retry.role, WorkerRole::TestWriter);
    match retry.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("Audit feedback"));
            assert!(prompt.contains("Missing edge-case assertions"));
        }
        JobRun::DeterministicTestRun => panic!("expected test writer prompt"),
    }
}

#[test]
fn multiple_implementor_audits_run_sequentially() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Do work".to_string(),
            details: "task details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "audit-1".to_string(),
            title: "Audit One".to_string(),
            details: "audit one details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "audit-2".to_string(),
            title: "Audit Two".to_string(),
            details: "audit two details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let implementor = wf.start_next_job().expect("implementor");
    assert_eq!(implementor.role, WorkerRole::Implementor);
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let first_audit = wf.start_next_job().expect("first audit");
    assert_eq!(first_audit.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let second_audit = wf.start_next_job().expect("second audit");
    assert_eq!(second_audit.role, WorkerRole::Auditor);
}

#[test]
fn exhausted_implementor_audit_moves_to_next_audit() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Do work".to_string(),
            details: "task details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "audit-1".to_string(),
            title: "Audit One".to_string(),
            details: "audit one details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "audit-2".to_string(),
            title: "Audit Two".to_string(),
            details: "audit two details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    for pass in 1..=4 {
        let _ = wf.start_next_job().expect("audit");
        wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
        wf.append_active_output(format!("critical issue pass {pass}"));
        wf.finish_active_job(true, 0);
        if pass < 4 {
            let _ = wf.start_next_job().expect("implementor retry");
            wf.append_active_output(format!("fix pass {pass}"));
            wf.finish_active_job(true, 0);
        }
    }

    let next_audit = wf.start_next_job().expect("next audit after exhaustion");
    assert_eq!(next_audit.role, WorkerRole::Auditor);
}

#[test]
fn started_jobs_keep_same_parent_context_key_per_branch() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let implementor = wf.start_next_job().expect("implementor");
    let impl_key = implementor
        .parent_context_key
        .expect("implementor context key");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    let auditor_key = auditor
        .parent_context_key
        .clone()
        .expect("auditor context key");
    assert_ne!(auditor_key, impl_key);
    wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
    wf.append_active_output("Fix needed".to_string());
    wf.finish_active_job(true, 0);

    let implementor_retry = wf.start_next_job().expect("implementor retry");
    assert_eq!(
        implementor_retry.parent_context_key.as_deref(),
        Some(impl_key.as_str())
    );
    wf.append_active_output("implemented retry".to_string());
    wf.finish_active_job(true, 0);

    let auditor_retry = wf.start_next_job().expect("auditor retry");
    assert_eq!(
        auditor_retry.parent_context_key.as_deref(),
        Some(auditor_key.as_str())
    );
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let test_writer = wf.start_next_job().expect("test writer");
    let tw_key = test_writer
        .parent_context_key
        .expect("test writer context key");
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner");
    assert_eq!(runner.parent_context_key.as_deref(), Some(tw_key.as_str()));
}

#[test]
fn test_fix_loop_stops_after_five_failed_test_runs() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("auditor");
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let mut last_messages = Vec::new();
    for pass in 1..=5 {
        let runner = wf.start_next_job().expect("test runner");
        assert_eq!(runner.role, WorkerRole::TestRunner);
        wf.append_active_output(format!("test run {pass} failed"));
        last_messages = wf.finish_active_job(false, 101);

        if pass < 5 {
            assert!(
                last_messages
                    .iter()
                    .any(|m| m.contains(&format!("test-writer pass {} queued", pass + 1)))
            );
            let writer = wf.start_next_job().expect("test writer retry");
            assert_eq!(writer.role, WorkerRole::TestWriter);
            wf.append_active_output(format!("updated tests pass {}", pass + 1));
            wf.finish_active_job(true, 0);
        }
    }

    assert!(
        last_messages
            .iter()
            .any(|m| m.contains("queued cleanup removal pass"))
    );
    let cleanup = wf.start_next_job().expect("cleanup writer pass");
    assert_eq!(cleanup.role, WorkerRole::TestWriter);
    match cleanup.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("Remove the failing tests completely"));
        }
        JobRun::DeterministicTestRun => panic!("expected cleanup writer prompt"),
    }
    wf.append_active_output("Removed failing tests".to_string());
    wf.finish_active_job(true, 0);
    assert!(wf.start_next_job().is_none());
    let tree = wf.right_pane_lines().join("\n");
    assert!(tree.contains("[x] Task: Do work"));
    let failures = wf.drain_recent_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].kind, WorkflowFailureKind::Test);
    assert_eq!(failures[0].attempts, 5);
}

#[test]
fn auditor_output_is_forwarded_to_implementor_retry_prompt() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("Issue: missing edge-case handling".to_string());
    let messages = wf.finish_active_job(true, 0);
    assert!(messages.iter().any(|m| m.contains("audit requested fixes")));

    let retry = wf.start_next_job().expect("implementor retry");
    assert_eq!(retry.role, WorkerRole::Implementor);
    match retry.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("Audit feedback"));
            assert!(prompt.contains("Issue: missing edge-case handling"));
        }
        JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
    }
}

#[test]
fn implementor_changed_files_summary_is_forwarded_to_auditor_prompt() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("Implemented feature updates.".to_string());
    wf.append_active_output("FILES_CHANGED_BEGIN".to_string());
    wf.append_active_output(
        "- src/app.rs: added state transition for command handling".to_string(),
    );
    wf.append_active_output(
        "- src/ui.rs: updated rendering path for task block layout".to_string(),
    );
    wf.append_active_output("FILES_CHANGED_END".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    match auditor.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("Implementor changed-files summary"));
            assert!(prompt.contains("AUDIT_RESULT: PASS"));
            assert!(prompt.contains("AUDIT_RESULT: FAIL"));
            assert!(prompt.contains("- src/app.rs: added state transition for command handling"));
            assert!(prompt.contains("- src/ui.rs: updated rendering path for task block layout"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
}

#[test]
fn audit_result_token_pass_overrides_issue_keywords() {
    assert!(!audit_detects_issues(&[
        "AUDIT_RESULT: PASS".to_string(),
        "Issue: wording only".to_string(),
    ]));
}

#[test]
fn audit_result_token_fail_overrides_no_issues_phrase() {
    assert!(audit_detects_issues(&[
        "AUDIT_RESULT: FAIL".to_string(),
        "No issues found".to_string(),
    ]));
}

#[test]
fn audit_result_token_is_detected_even_after_preamble_lines() {
    assert!(!audit_detects_issues(&[
        "Summary of findings".to_string(),
        "AUDIT_RESULT: PASS".to_string(),
        "Issue: wording only".to_string(),
    ]));
}

#[test]
fn missing_audit_result_token_falls_back_to_heuristics() {
    assert!(audit_detects_issues(&[
        "Issue: edge case missing".to_string()
    ]));
    assert!(!audit_detects_issues(&["No issues found".to_string()]));
}

#[test]
fn audit_strictness_policy_relaxes_by_pass() {
    assert!(audit_strictness_policy(1).contains("strict"));
    assert!(audit_strictness_policy(2).contains("moderate"));
    assert!(audit_strictness_policy(3).contains("high-impact"));
    assert!(audit_strictness_policy(4).contains("critical only"));
    assert!(audit_strictness_policy(8).contains("critical only"));
}

#[test]
fn audit_retry_limit_stops_after_four_failed_audits() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor pass 1");
    wf.append_active_output("implemented v1".to_string());
    wf.finish_active_job(true, 0);

    let mut last_messages = Vec::new();
    for audit_pass in 1..=4 {
        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        match auditor.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains(&format!("Audit pass: {audit_pass} of 4")));
                if audit_pass == 4 {
                    assert!(prompt.contains("truly critical blockers"));
                }
            }
            JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
        }
        wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
        wf.append_active_output("Critical blocker still present".to_string());
        last_messages = wf.finish_active_job(true, 0);

        if audit_pass < 4 {
            let implementor = wf.start_next_job().expect("implementor retry");
            assert_eq!(implementor.role, WorkerRole::Implementor);
            wf.append_active_output(format!("implemented retry {audit_pass}"));
            wf.finish_active_job(true, 0);
        }
    }

    assert!(
        last_messages
            .iter()
            .any(|m| m.contains(&format!("Max retries ({MAX_AUDIT_RETRIES}) reached")))
    );

    let writer = wf.start_next_job().expect("test writer after audit exhaustion");
    assert_eq!(writer.role, WorkerRole::TestWriter);
    wf.append_active_output("wrote tests after exhausted audit".to_string());
    wf.finish_active_job(true, 0);

    let runner = wf.start_next_job().expect("test runner after audit exhaustion");
    assert_eq!(runner.role, WorkerRole::TestRunner);
    wf.append_active_output("all passed".to_string());
    wf.finish_active_job(true, 0);

    assert!(wf.start_next_job().is_none());
    let tree = wf.right_pane_lines().join("\n");
    assert!(tree.contains("[x] Task: Do work"));
    let failures = wf.drain_recent_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].kind, WorkflowFailureKind::Audit);
    assert_eq!(failures[0].attempts, 4);
}

#[test]
fn implementor_owned_test_runner_runs_after_audits_when_present() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "t1".to_string(),
            title: "Do work".to_string(),
            details: "task details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl1".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("t1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "runner1".to_string(),
            title: "Existing Tests".to_string(),
            details: "run existing suite".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl1".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "audit1".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl1".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let implementor = wf.start_next_job().expect("implementor");
    assert_eq!(implementor.role, WorkerRole::Implementor);
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let existing_runner = wf.start_next_job().expect("existing runner");
    assert_eq!(existing_runner.role, WorkerRole::TestRunner);
    assert!(matches!(existing_runner.run, JobRun::DeterministicTestRun));
    wf.append_active_output("existing tests passed".to_string());
    wf.finish_active_job(true, 0);

    let tree = wf.right_pane_lines().join("\n");
    assert!(tree.contains("[x] Task: Do work"));
}

#[test]
fn implementor_owned_test_runner_exhaustion_records_failure_and_continues_to_next_step() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "t1".to_string(),
            title: "Do work".to_string(),
            details: "task details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl1".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("t1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "runner1".to_string(),
            title: "Existing Tests".to_string(),
            details: "run existing suite".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl1".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "audit1".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl1".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let implementor = wf.start_next_job().expect("implementor");
    assert_eq!(implementor.role, WorkerRole::Implementor);
    wf.append_active_output("implemented initial".to_string());
    wf.finish_active_job(true, 0);

    let auditor = wf.start_next_job().expect("auditor");
    assert_eq!(auditor.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    for pass in 1..=5 {
        let existing_runner = wf.start_next_job().expect("existing runner");
        assert_eq!(existing_runner.role, WorkerRole::TestRunner);
        wf.append_active_output(format!("existing tests failed pass {pass}"));
        let messages = wf.finish_active_job(false, 101);
        if pass == 5 {
            assert!(
                messages
                    .iter()
                    .any(|m| m.contains("Max retries (5) reached"))
            );
        } else {
            let implementor = wf.start_next_job().expect("implementor retry");
            assert_eq!(implementor.role, WorkerRole::Implementor);
            wf.append_active_output(format!("implemented pass {}", pass + 1));
            wf.finish_active_job(true, 0);
        }
    }

    let failures = wf.drain_recent_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].kind, WorkflowFailureKind::Test);
    assert_eq!(failures[0].attempts, 5);
    let tree = wf.right_pane_lines().join("\n");
    assert!(tree.contains("[x] Task: Do work"));
}

#[test]
fn top_task_done_requires_impl_and_test_branches() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();

    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("wrote tests".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("auditor");
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let tree = wf.right_pane_lines().join("\n");
    assert!(!tree.contains("[x] Task: Do work"));

    let _ = wf.start_next_job().expect("test runner");
    wf.append_active_output("all passed".to_string());
    wf.finish_active_job(true, 0);

    let tree = wf.right_pane_lines().join("\n");
    assert!(tree.contains("[x] Task: Do work"));
}

#[test]
fn sync_rejects_tasks_missing_details() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
            id: "t1".to_string(),
            title: "Task".to_string(),
            details: "".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        }])
        .expect_err("missing details should fail");
    assert!(err.contains("non-empty details"));
}

#[test]
fn sync_rejects_reload_while_execution_has_active_or_queued_work() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();
    assert!(wf.execution_busy());

    let err = wf
        .sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
            id: "new-top".to_string(),
            title: "New top".to_string(),
            details: "details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        }])
        .expect_err("reload should be blocked while execution is busy");
    assert!(err.contains("Cannot reload planner tasks while execution is enabled"));
}

#[test]
fn sync_allows_reload_when_execution_is_enabled_but_idle() {
    let mut wf = Workflow::default();
    wf.start_execution();
    assert!(wf.execution_enabled());
    assert!(!wf.execution_busy());

    let count = wf
        .sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        }])
        .expect("reload should succeed when execution is idle");
    assert_eq!(count, 1);
    assert!(!wf.execution_enabled());
}

#[test]
fn auto_generated_subtasks_include_non_empty_details_for_reload() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
        id: "top".to_string(),
        title: "Top task".to_string(),
        details: "top details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }])
    .expect("seed plan should sync");

    wf.start_execution();
    let _ = wf.start_next_job().expect("implementor should start");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer should start");
    wf.append_active_output("tests written".to_string());
    wf.finish_active_job(true, 0);

    let snapshot = wf.planner_tasks_for_file();
    assert!(
        snapshot
            .iter()
            .all(|entry| !entry.details.trim().is_empty())
    );

    let mut reloaded = Workflow::default();
    reloaded
        .sync_planner_tasks_from_file(snapshot)
        .expect("snapshot should be reloadable");
}

#[test]
fn mid_run_snapshot_with_auto_generated_branches_is_reloadable() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
        id: "top".to_string(),
        title: "Top task".to_string(),
        details: "top details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }])
    .expect("seed plan should sync");

    wf.start_execution();
    let _ = wf.start_next_job().expect("implementor should start");

    let snapshot = wf.planner_tasks_for_file();
    assert!(
        snapshot
            .iter()
            .any(|entry| entry.kind == PlannerTaskKindFile::TestWriter)
    );
    assert!(
        snapshot
            .iter()
            .any(|entry| entry.kind == PlannerTaskKindFile::TestRunner)
    );

    let mut reloaded = Workflow::default();
    reloaded
        .sync_planner_tasks_from_file(snapshot)
        .expect("mid-run snapshot should remain reloadable");
}

#[test]
fn sync_rejects_implementor_without_auditor() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
        ])
        .expect_err("should reject missing auditor");
    assert!(err.contains("must include at least one auditor"));
}

#[test]
fn sync_rejects_implementor_test_runner_before_audit() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-runner".to_string(),
                title: "Runner".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject runner before audit");
    assert!(err.contains("test_runner must come after audit"));
}

#[test]
fn sync_rejects_test_writer_without_test_runner() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw".to_string(),
                title: "Tests".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject missing test runner");
    assert!(err.contains("must include at least one test_runner"));
}

#[test]
fn sync_rejects_nested_test_writer_groups() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tests-parent".to_string(),
                title: "Tests".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tests-parent-runner".to_string(),
                title: "Run parent".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tests-parent".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tests-child".to_string(),
                title: "Nested tests".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tests-parent".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tests-child-runner".to_string(),
                title: "Run child".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tests-child".to_string()),
                order: Some(0),
            },
        ])
        .expect_err("should reject nested test writer grouping");
    assert!(err.contains("must be a direct child of a top-level task"));
}

#[test]
fn sync_rejects_nested_implementor_branches() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-root".to_string(),
                title: "Impl root".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-root-audit".to_string(),
                title: "Impl root audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl-root".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-nested".to_string(),
                title: "Impl nested".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl-root".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "impl-nested-audit".to_string(),
                title: "Impl nested audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl-nested".to_string()),
                order: Some(0),
            },
        ])
        .expect_err("should reject nested implementor branch");
    assert!(err.contains("Implementor task \"impl-nested\""));
    assert!(err.contains("must be a direct child of a top-level task"));
}

#[test]
fn sync_rejects_nested_final_audit_task() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "nested-final".to_string(),
                title: "Final".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::FinalAudit,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject nested final audit task");
    assert!(err.contains("Final-audit task \"nested-final\""));
    assert!(err.contains("must be a top-level task"));
}

#[test]
fn sync_rejects_auditor_with_invalid_parent_kind() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "orphan-audit".to_string(),
                title: "Orphan Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject auditor parent kind");
    assert!(err.contains("Auditor task"));
    assert!(err.contains("child of implementor or test_writer"));
}

#[test]
fn sync_rejects_test_runner_with_invalid_parent_kind() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "orphan-runner".to_string(),
                title: "Orphan Runner".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject test-runner parent kind");
    assert!(err.contains("Test-runner task"));
    assert!(err.contains("child of implementor or test_writer"));
}

#[test]
fn sync_rejects_multiple_test_runners_under_implementor() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-runner-1".to_string(),
                title: "Runner 1".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "impl-runner-2".to_string(),
                title: "Runner 2".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(2),
            },
        ])
        .expect_err("should reject multiple implementor test runners");
    assert!(err.contains("Implementor task"));
    assert!(err.contains("at most one test_runner"));
}

#[test]
fn sync_rejects_multiple_test_runners_under_test_writer() {
    let mut wf = Workflow::default();
    let err = wf
        .sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Top".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl Audit".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw".to_string(),
                title: "Tests".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw-runner-1".to_string(),
                title: "Runner 1".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw-runner-2".to_string(),
                title: "Runner 2".to_string(),
                details: "d".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(1),
            },
        ])
        .expect_err("should reject multiple test-writer test runners");
    assert!(err.contains("Test-writer task"));
    assert!(err.contains("at most one test_runner"));
}

#[test]
fn implementor_and_auditor_prompts_include_test_guardrails() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();
    let implementor = wf.start_next_job().expect("implementor");
    match implementor.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("do not create or modify tests unless"));
            assert!(prompt.contains("Implementation details:"));
            assert!(prompt.contains("implementor details"));
        }
        JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
    }

    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);
    let auditor = wf.start_next_job().expect("auditor");
    match auditor.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("do not audit test quality/coverage"));
            assert!(prompt.contains("Parent implementor details:"));
            assert!(prompt.contains("implementor details"));
            assert!(prompt.contains("Scope lock (required): audit only the parent implementor"));
            assert!(prompt.contains("do not run tests"));
            assert!(prompt.contains("do not execute/check shell commands"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
}

#[test]
fn worker_prompts_prepend_task_docs_and_web_read_instruction() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Do work".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Impl guide".to_string(),
                url: "https://docs.example/impl".to_string(),
                summary: "Implementation guidance".to_string(),
            }],
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Implementation audit".to_string(),
            details: "audit details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Audit guide".to_string(),
                url: "https://docs.example/impl-audit".to_string(),
                summary: "Audit guidance".to_string(),
            }],
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Testing guide".to_string(),
                url: "https://docs.example/test-writer".to_string(),
                summary: "Test-writing guidance".to_string(),
            }],
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-audit".to_string(),
            title: "Test audit".to_string(),
            details: "test audit details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Test audit guide".to_string(),
                url: "https://docs.example/test-audit".to_string(),
                summary: "Test audit guidance".to_string(),
            }],
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "final".to_string(),
            title: "Final audit".to_string(),
            details: "final audit details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Final review guide".to_string(),
                url: "https://docs.example/final-audit".to_string(),
                summary: "Final audit guidance".to_string(),
            }],
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let mut saw_impl = false;
    let mut saw_impl_auditor = false;
    let mut saw_test_writer = false;
    let mut saw_test_auditor = false;
    let mut saw_final_audit = false;

    for _ in 0..24 {
        let Some(job) = wf.start_next_job() else {
            break;
        };
        match &job.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.starts_with("Task documentation requirements:"));
                assert!(prompt.contains("read every linked document from the web"));
                if prompt.contains("You are an implementation sub-agent.") {
                    saw_impl = true;
                    assert!(prompt.contains("https://docs.example/impl"));
                } else if prompt.contains("reviewing implementation output") {
                    saw_impl_auditor = true;
                    assert!(prompt.contains("https://docs.example/impl-audit"));
                } else if prompt.contains("You are a test-writer sub-agent.") {
                    saw_test_writer = true;
                    assert!(prompt.contains("https://docs.example/test-writer"));
                } else if prompt.contains("reviewing test-writing output") {
                    saw_test_auditor = true;
                    assert!(prompt.contains("https://docs.example/test-audit"));
                } else if prompt.contains("You are a final audit sub-agent.") {
                    saw_final_audit = true;
                    assert!(prompt.contains("https://docs.example/final-audit"));
                } else {
                    panic!("unexpected prompt variant: {prompt}");
                }
            }
            JobRun::DeterministicTestRun => {}
        }

        match job.role {
            WorkerRole::Auditor | WorkerRole::FinalAudit => {
                wf.append_active_output("AUDIT_RESULT: PASS".to_string());
                wf.append_active_output("No issues found".to_string());
            }
            WorkerRole::TestRunner => {
                wf.append_active_output("all passed".to_string());
            }
            _ => wf.append_active_output("completed".to_string()),
        }
        wf.finish_active_job(true, 0);
    }

    assert!(saw_impl, "implementor prompt was not observed");
    assert!(
        saw_impl_auditor,
        "implementor auditor prompt was not observed"
    );
    assert!(saw_test_writer, "test-writer prompt was not observed");
    assert!(
        saw_test_auditor,
        "test-writer auditor prompt was not observed"
    );
    assert!(saw_final_audit, "final-audit prompt was not observed");
}

#[test]
fn start_execution_picks_unfinished_task_when_some_are_done() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "done-task".to_string(),
            title: "Done task".to_string(),
            details: "already done".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Done,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "todo-task".to_string(),
            title: "Pending task".to_string(),
            details: "needs execution".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let first = wf.start_next_job().expect("should start unfinished task");
    assert_eq!(first.role, WorkerRole::Implementor);
    match first.run {
        JobRun::AgentPrompt(prompt) => assert!(prompt.contains("Pending task")),
        JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
    }
}

#[test]
fn start_execution_resumes_with_pending_implementor_auditor_subtask() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "tw details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Done,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let started = wf.start_next_job().expect("should resume pending auditor");
    assert_eq!(started.role, WorkerRole::Auditor);
    match started.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("reviewing implementation output"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
}

#[test]
fn start_execution_resumes_with_in_progress_implementor_and_pending_auditor_subtask() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::InProgress,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let started = wf.start_next_job().expect("should resume pending auditor");
    assert_eq!(started.role, WorkerRole::Auditor);
    match started.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("reviewing implementation output"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
}

#[test]
fn first_implementor_pass_enqueues_implementor_auditor_before_test_writer() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-runner".to_string(),
            title: "Run tests".to_string(),
            details: "runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let first = wf.start_next_job().expect("implementor");
    assert_eq!(first.role, WorkerRole::Implementor);
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let second = wf.start_next_job().expect("implementor auditor");
    assert_eq!(second.role, WorkerRole::Auditor);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("looks good".to_string());
    wf.finish_active_job(true, 0);

    let third = wf.start_next_job().expect("implementor test runner");
    assert_eq!(third.role, WorkerRole::TestRunner);
}

#[test]
fn resume_does_not_mark_task_done_with_pending_implementor_auditor() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top-a".to_string(),
            title: "Top A".to_string(),
            details: "top A details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top-a-impl".to_string(),
            title: "Top A implementation".to_string(),
            details: "top A implementation".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-a".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top-a-audit".to_string(),
            title: "Top A audit".to_string(),
            details: "top A audit".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-a-impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top-a-tw".to_string(),
            title: "Top A write tests".to_string(),
            details: "top A test writer details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-a".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "top-a-tw-runner".to_string(),
            title: "Top A run tests".to_string(),
            details: "top A test runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-a-tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top-b".to_string(),
            title: "Top B".to_string(),
            details: "top B details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "top-b-impl".to_string(),
            title: "Top B implementation".to_string(),
            details: "top B implementation".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-b".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "top-b-audit".to_string(),
            title: "Top B audit".to_string(),
            details: "top B audit".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-b-impl".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let _ = wf.start_next_job().expect("top A implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let snapshot = wf.planner_tasks_for_file();
    let top_status = snapshot
        .iter()
        .find(|entry| entry.id == "top-a")
        .map(|entry| entry.status);
    assert_ne!(top_status, Some(PlannerTaskStatusFile::Done));

    let mut resumed = Workflow::default();
    resumed.sync_planner_tasks_from_file(snapshot).expect("reload should succeed");
    resumed.start_execution();
    let next = resumed.start_next_job().expect("next job should resume pending work on top A");
    assert_eq!(next.role, WorkerRole::Auditor);
    match next.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("reviewing implementation output"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }
}

#[test]
fn enqueue_ready_does_not_mutate_done_or_final_audit_roots() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "done".to_string(),
            title: "Already done".to_string(),
            details: "done details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Done,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "fa".to_string(),
            title: "Final Audit".to_string(),
            details: "final audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();

    let snapshot = wf.planner_tasks_for_file();
    assert!(
        !snapshot
            .iter()
            .any(|entry| entry.parent_id.as_deref() == Some("done"))
    );
    assert!(
        !snapshot
            .iter()
            .any(|entry| entry.parent_id.as_deref() == Some("fa"))
    );

    let final_job = wf.start_next_job().expect("final audit should be queued");
    assert_eq!(final_job.role, WorkerRole::FinalAudit);
}

#[test]
fn planner_tasks_for_file_reflects_runtime_status_transitions() {
    let mut wf = Workflow::default();
    seed_single_default_task(&mut wf, "Do work");
    wf.start_execution();
    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let snapshot = wf.planner_tasks_for_file();
    let impl_status = snapshot
        .iter()
        .find(|entry| entry.id == "impl")
        .map(|entry| entry.status);
    assert_eq!(impl_status, Some(PlannerTaskStatusFile::Done));

    let audit_status = snapshot
        .iter()
        .find(|entry| entry.id == "impl-audit")
        .map(|entry| entry.status);
    assert_eq!(audit_status, Some(PlannerTaskStatusFile::Pending));
}

#[test]
fn final_audit_runs_after_non_final_tasks_complete() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "t1".to_string(),
            title: "Do work".to_string(),
            details: "task details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "fa".to_string(),
            title: "Final Audit".to_string(),
            details: "final audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    wf.start_execution();
    let _ = wf.start_next_job().expect("implementor");
    wf.append_active_output("implemented".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test writer");
    wf.append_active_output("tests".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("auditor");
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let _ = wf.start_next_job().expect("test runner");
    wf.append_active_output("all passed".to_string());
    wf.finish_active_job(true, 0);

    let final_job = wf.start_next_job().expect("final audit");
    assert_eq!(final_job.role, WorkerRole::FinalAudit);
}

#[test]
fn final_audit_audit_result_fail_keeps_task_needs_changes_even_on_exit_zero() {
    let mut wf = Workflow::default();
    seed_single_default_task_with_final_audit(&mut wf, "Do work");
    let _ = complete_non_final_branches_and_start_final_audit(&mut wf);

    wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
    wf.append_active_output("Cross-task issue still unresolved".to_string());
    let messages = wf.finish_active_job(true, 0);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("did not explicitly pass; retry queued")),
        "expected retry message when final audit returns AUDIT_RESULT: FAIL"
    );

    let snapshot = wf.planner_tasks_for_file();
    let final_status = snapshot
        .iter()
        .find(|entry| entry.id == "fa")
        .map(|entry| entry.status);
    assert_eq!(final_status, Some(PlannerTaskStatusFile::NeedsChanges));
}

#[test]
fn final_audit_requires_explicit_pass_token_to_complete() {
    let mut wf = Workflow::default();
    seed_single_default_task_with_final_audit(&mut wf, "Do work");
    let first_final_audit = complete_non_final_branches_and_start_final_audit(&mut wf);
    match first_final_audit.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("AUDIT_RESULT: PASS"));
            assert!(prompt.contains("AUDIT_RESULT: FAIL"));
        }
        JobRun::DeterministicTestRun => panic!("expected final audit prompt"),
    }

    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);
    let first_snapshot = wf.planner_tasks_for_file();
    let first_status = first_snapshot
        .iter()
        .find(|entry| entry.id == "fa")
        .map(|entry| entry.status);
    assert_eq!(first_status, Some(PlannerTaskStatusFile::NeedsChanges));

    let retry_final_audit = wf.start_next_job().expect("final audit retry");
    assert_eq!(retry_final_audit.role, WorkerRole::FinalAudit);
    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
    wf.append_active_output("No issues found".to_string());
    wf.finish_active_job(true, 0);

    let second_snapshot = wf.planner_tasks_for_file();
    let second_status = second_snapshot
        .iter()
        .find(|entry| entry.id == "fa")
        .map(|entry| entry.status);
    assert_eq!(second_status, Some(PlannerTaskStatusFile::Done));
}

#[test]
fn final_audit_retry_limit_stops_requeue_and_records_failure() {
    let mut wf = Workflow::default();
    seed_single_default_task_with_final_audit(&mut wf, "Do work");
    let _ = complete_non_final_branches_and_start_final_audit(&mut wf);

    let mut last_messages = Vec::new();
    for pass in 1..=MAX_FINAL_AUDIT_RETRIES {
        wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
        wf.append_active_output(format!("cross-task blocker remains on pass {pass}"));
        last_messages = wf.finish_active_job(true, 0);

        if pass < MAX_FINAL_AUDIT_RETRIES {
            let retry = wf.start_next_job().expect("final audit retry");
            assert_eq!(retry.role, WorkerRole::FinalAudit);
        }
    }

    assert!(
        last_messages
            .iter()
            .any(|m| m.contains(&format!("Max retries ({MAX_FINAL_AUDIT_RETRIES}) reached")))
    );
    assert!(wf.start_next_job().is_none());

    let failures = wf.drain_recent_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].kind, WorkflowFailureKind::Audit);
    assert_eq!(failures[0].attempts, MAX_FINAL_AUDIT_RETRIES);
    assert!(failures[0].action_taken.contains("stopped requeueing"));

    let snapshot = wf.planner_tasks_for_file();
    let final_status = snapshot
        .iter()
        .find(|entry| entry.id == "fa")
        .map(|entry| entry.status);
    assert_eq!(final_status, Some(PlannerTaskStatusFile::NeedsChanges));
}

#[test]
fn replace_rolling_context_entries_trims_to_limit() {
    let mut wf = Workflow::default();
    let entries = (0..40)
        .map(|idx| format!("entry-{idx}"))
        .collect::<Vec<_>>();
    wf.replace_rolling_context_entries(entries);
    let loaded = wf.rolling_context_entries();
    assert_eq!(loaded.len(), 16);
    assert_eq!(loaded.first().map(String::as_str), Some("entry-24"));
    assert_eq!(loaded.last().map(String::as_str), Some("entry-39"));
}

#[test]
fn right_pane_shows_docs_badge_for_non_test_runner_only() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "task".to_string(),
            title: "Task".to_string(),
            details: "Top details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Doc".to_string(),
                url: "https://example.com".to_string(),
                summary: "summary".to_string(),
            }],
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "Implement".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("task".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "Audit implementation".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "runner".to_string(),
            title: "Runner".to_string(),
            details: "Runner details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Should hide".to_string(),
                url: "https://example.com".to_string(),
                summary: "summary".to_string(),
            }],
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(1),
        },
    ])
    .expect("sync should succeed");
    let text = wf
        .right_pane_block_view(80, &HashSet::new())
        .lines
        .join("\n");
    assert!(text.contains("[documentation attached]"));
    assert_eq!(text.matches("[documentation attached]").count(), 1);
}

#[test]
fn right_pane_docs_toggle_expands_full_docs_without_collapsed_preview() {
    let mut wf = Workflow::default();
    wf.sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
        id: "task".to_string(),
        title: "Task".to_string(),
        details: "Top details".to_string(),
        docs: vec![PlannerTaskDocFileEntry {
            title: "Doc Title".to_string(),
            url: "https://example.com/doc".to_string(),
            summary: "Doc summary".to_string(),
        }],
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }])
    .expect("sync should succeed");

    let collapsed = wf
        .right_pane_block_view(80, &HashSet::new())
        .lines
        .join("\n");
    assert!(collapsed.contains("[documentation attached] [+]"));
    assert!(!collapsed.contains("Doc Title"));
    assert!(!collapsed.contains("https://example.com/doc"));
    assert!(!collapsed.contains("Doc summary"));

    let mut expanded = HashSet::new();
    expanded.insert(docs_toggle_key("task"));
    let expanded_text = wf.right_pane_block_view(80, &expanded).lines.join("\n");
    assert!(expanded_text.contains("[documentation attached] [-]"));
    assert!(expanded_text.contains("Doc Title"));
    assert!(expanded_text.contains("https://example.com/doc"));
    assert!(expanded_text.contains("Doc summary"));
}
