use super::Workflow;
use super::{make_context_summary, test_runner_feedback, TaskStatus};

pub(crate) fn on_writer_completion(
    workflow: &mut Workflow,
    top_task_id: u64,
    test_writer_id: u64,
    test_runner_id: u64,
    pass: u8,
    transcript: &[String],
    success: bool,
    code: i32,
    messages: &mut Vec<String>,
) {
    workflow.set_status(test_runner_id, TaskStatus::Done);
    workflow.push_context(make_context_summary(
        "TestRunner",
        &workflow.task_title(top_task_id),
        transcript,
        success,
    ));

    if success {
        workflow.set_status(test_writer_id, TaskStatus::Done);
        messages.push(format!(
            "System: Task #{} deterministic tests passed on run {}.",
            top_task_id, pass
        ));
        workflow.try_mark_top_done(top_task_id, messages);
    } else {
        workflow.set_status(test_writer_id, TaskStatus::NeedsChanges);
        if pass >= super::MAX_TEST_RETRIES {
            let failure_reason = test_runner_feedback(transcript, code);
            workflow.recent_failures.push(super::WorkflowFailure {
                kind: super::WorkflowFailureKind::Test,
                top_task_id,
                top_task_title: workflow.task_title(top_task_id),
                attempts: pass,
                reason: failure_reason.clone(),
                action_taken: "Requested test cleanup (remove failing tests) and continued."
                    .to_string(),
            });
            workflow.queue.push_back(super::WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::TestWriter {
                    test_writer_id,
                    pass: pass.saturating_add(1),
                    feedback: Some(format!(
                        "Deterministic test retries exhausted.\n\
                         Remove the failing tests completely so they no longer fail.\n\
                         Do not add replacement tests in this pass.\n\
                         Then report exactly which tests/files were removed.\n\
                         Failure details:\n{}",
                        failure_reason
                    )),
                    skip_test_runner_on_success: true,
                    resume_auditor_id: None,
                    resume_audit_pass: None,
                },
            });
            messages.push(format!(
                "System: Task #{} tests still failing at pass {}. Max retries ({}) reached; queued cleanup removal pass.",
                top_task_id, pass, super::MAX_TEST_RETRIES
            ));
        } else {
            workflow.queue.push_back(super::WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::TestWriter {
                    test_writer_id,
                    pass: pass.saturating_add(1),
                    feedback: Some(test_runner_feedback(transcript, code)),
                    skip_test_runner_on_success: false,
                    resume_auditor_id: None,
                    resume_audit_pass: None,
                },
            });
            messages.push(format!(
                "System: Task #{} tests failed; test-writer pass {} queued.",
                top_task_id,
                pass.saturating_add(1)
            ));
        }
    }
}

pub(crate) fn on_implementor_completion(
    workflow: &mut Workflow,
    top_task_id: u64,
    implementor_id: u64,
    test_runner_id: u64,
    pass: u8,
    transcript: &[String],
    success: bool,
    code: i32,
    messages: &mut Vec<String>,
) {
    workflow.push_context(make_context_summary(
        "TestRunner",
        &workflow.task_title(top_task_id),
        transcript,
        success,
    ));

    if success {
        workflow.set_status(test_runner_id, TaskStatus::Done);
        workflow.set_status(implementor_id, TaskStatus::Done);
        workflow.try_mark_top_done(top_task_id, messages);
        messages.push(format!(
            "System: Task #{} existing-test runner passed on run {}; implementor branch complete.",
            top_task_id, pass
        ));
    } else {
        workflow.set_status(test_runner_id, TaskStatus::NeedsChanges);
        if pass >= super::MAX_TEST_RETRIES {
            workflow.set_status(test_runner_id, TaskStatus::Done);
            workflow.recent_failures.push(super::WorkflowFailure {
                kind: super::WorkflowFailureKind::Test,
                top_task_id,
                top_task_title: workflow.task_title(top_task_id),
                attempts: pass,
                reason: test_runner_feedback(transcript, code),
                action_taken:
                    "Existing-tests runner retries exhausted; continued to next step.".to_string(),
            });
            workflow.set_status(implementor_id, TaskStatus::Done);
            workflow.try_mark_top_done(top_task_id, messages);
            messages.push(format!(
                "System: Task #{} existing tests still failing at pass {}. Max retries ({}) reached; proceeding to next step.",
                top_task_id, pass, super::MAX_TEST_RETRIES
            ));
        } else {
            workflow.set_status(implementor_id, TaskStatus::NeedsChanges);
            workflow.queue.push_back(super::WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::Implementor {
                    implementor_id,
                    pass: pass.saturating_add(1),
                    feedback: Some(test_runner_feedback(transcript, code)),
                    resume_auditor_id: None,
                    resume_audit_pass: None,
                },
            });
            messages.push(format!(
                "System: Task #{} existing tests failed; implementor pass {} queued.",
                top_task_id,
                pass.saturating_add(1)
            ));
        }
    }
}
