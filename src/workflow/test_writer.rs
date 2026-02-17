use super::{make_context_summary, MAX_TEST_RETRIES, TaskStatus, WorkerJob, Workflow};

pub(crate) fn build_prompt(
    workflow: &Workflow,
    top_task_id: u64,
    test_writer_id: u64,
    feedback: Option<&str>,
    skip_test_runner_on_success: bool,
) -> String {
    format!(
        "You are a test-writer sub-agent.\n\
         Top-level task: {}\n\
         Test-writer subtask: {}\n\
         Test-writing details:\n{}\n\
         Rolling task context:\n{}\n\
         {}\n\
         Write or update tests to cover current implementation thoroughly.\n\
         {}\n\
         Keep output concise and include what test behavior was added.",
        workflow.task_title(top_task_id),
        workflow.node_title(test_writer_id, "Test Writing"),
        workflow.node_details(test_writer_id),
        workflow.context_block(),
        feedback
            .as_ref()
            .map(|f| format!("Feedback to address before re-running deterministic tests:\n{f}"))
            .unwrap_or_else(|| "No test feedback yet; infer tests from task and implementation branch progress.".to_string()),
        if skip_test_runner_on_success {
            "Special instruction: this is a cleanup pass after exhausted deterministic test retries. Remove failing tests and do not add replacements."
        } else {
            ""
        }
    )
}

pub(crate) fn on_completion(
    workflow: &mut Workflow,
    top_task_id: u64,
    test_writer_id: u64,
    pass: u8,
    skip_test_runner_on_success: bool,
    resume_auditor_id: Option<u64>,
    resume_audit_pass: Option<u8>,
    transcript: &[String],
    success: bool,
    code: i32,
    messages: &mut Vec<String>,
) {
    workflow.push_context(make_context_summary(
        "TestWriter",
        &workflow.task_title(top_task_id),
        transcript,
        success,
    ));

    if success {
        if skip_test_runner_on_success {
            workflow.set_status(test_writer_id, TaskStatus::Done);
            messages.push(format!(
                "System: Task #{} removed failing tests after retries and proceeded.",
                top_task_id
            ));
            workflow.try_mark_top_done(top_task_id, messages);
            return;
        }
        if let Some(auditor_id) = resume_auditor_id {
            workflow.queue.push_back(WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::TestWriterAuditor {
                    test_writer_id,
                    auditor_id,
                    pass: resume_audit_pass.unwrap_or(1),
                    test_report: Some(transcript.join("\n")),
                },
            });
            messages.push(format!(
                "System: Task #{} test-writer pass {} complete; resumed audit queued.",
                top_task_id, pass
            ));
        } else {
            workflow.queue_test_writer_next_step(
                top_task_id,
                test_writer_id,
                pass,
                true,
                Some(transcript.join("\n")),
                messages,
            );
        }
    } else {
        workflow.set_status(test_writer_id, TaskStatus::NeedsChanges);
        if pass >= MAX_TEST_RETRIES {
            workflow.set_status(test_writer_id, TaskStatus::Done);
            workflow.recent_failures.push(super::WorkflowFailure {
                kind: super::WorkflowFailureKind::Test,
                top_task_id,
                top_task_title: workflow.task_title(top_task_id),
                attempts: pass,
                reason: format!("Test-writer failed repeatedly; latest exit code {code}."),
                action_taken: "Test-writer retries exhausted; proceeded without adding tests."
                    .to_string(),
            });
            messages.push(format!(
                "System: Task #{} test-writer still failing at pass {}. Max retries ({}) reached; proceeding to next step.",
                top_task_id, pass, MAX_TEST_RETRIES
            ));
            workflow.try_mark_top_done(top_task_id, messages);
        } else {
            workflow.queue.push_back(WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::TestWriter {
                    test_writer_id,
                    pass: pass.saturating_add(1),
                    feedback: Some(format!("Previous test-writer run failed with code {code}.")),
                    skip_test_runner_on_success: false,
                    resume_auditor_id: None,
                    resume_audit_pass: None,
                },
            });
            messages.push(format!(
                "System: Task #{} test-writer failed (code {}); retry queued.",
                top_task_id, code
            ));
        }
    }
}
