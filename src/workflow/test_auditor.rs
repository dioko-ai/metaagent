use super::Workflow;
use super::WorkerJob;
use super::{
    audit_detects_issues, audit_feedback, make_context_summary, TaskStatus,
};

pub(crate) fn build_prompt(
    workflow: &Workflow,
    top_task_id: u64,
    test_writer_id: u64,
    auditor_id: u64,
    test_report: &Option<String>,
    pass: u8,
) -> String {
    format!(
        "You are an audit sub-agent reviewing test-writing output.\n\
         Top-level task: {}\n\
         Parent test-writer task: {}\n\
         Parent test-writer details:\n{}\n\
         Audit subtask details:\n{}\n\
         Audit pass: {} of {}\n\
         Rolling task context:\n{}\n\
         Test-writer output to audit:\n{}\n\
         Focus on test quality, coverage relevance, and whether tests clearly validate intended behavior.\n\
         Execution guardrail: do not run tests and do not execute/check shell commands. Command/test execution is handled by a subsequent dedicated agent.\n\
         Strictness policy for this audit pass:\n{}\n\
         Response protocol (required):\n\
         - First line must be exactly one of:\n\
           AUDIT_RESULT: PASS\n\
           AUDIT_RESULT: FAIL\n\
         - Then provide concise findings. If PASS, include a brief rationale.\n\
         - If FAIL, include concrete issues and suggested fixes. On pass 4, only FAIL for truly critical blockers that would prevent the broader plan from running.",
        workflow.task_title(top_task_id),
        workflow.node_title(test_writer_id, "Test Writing"),
        workflow.node_details(test_writer_id),
        workflow.node_details(auditor_id),
        pass,
        super::MAX_AUDIT_RETRIES,
        workflow.context_block(),
        test_report
            .as_deref()
            .unwrap_or("(no test-writer output captured)"),
        audit_strictness_policy(pass),
    )
}

pub(crate) fn on_completion(
    workflow: &mut Workflow,
    top_task_id: u64,
    test_writer_id: u64,
    auditor_id: u64,
    pass: u8,
    test_report: Option<String>,
    transcript: &[String],
    success: bool,
    code: i32,
    messages: &mut Vec<String>,
) {
    workflow.push_context(make_context_summary(
        "Auditor",
        &workflow.task_title(top_task_id),
        transcript,
        success,
    ));

    let issues = !success || audit_detects_issues(transcript);
    if issues {
        workflow.set_status(test_writer_id, TaskStatus::NeedsChanges);
        if pass >= super::MAX_AUDIT_RETRIES {
            workflow.set_status(auditor_id, TaskStatus::Done);
            workflow.recent_failures.push(super::WorkflowFailure {
                kind: super::WorkflowFailureKind::Audit,
                top_task_id,
                top_task_title: workflow.task_title(top_task_id),
                attempts: pass,
                reason: audit_feedback(transcript, code, success),
                action_taken:
                    "Test-writer audit retries exhausted; continued to deterministic test run."
                        .to_string(),
            });
            workflow.queue_test_writer_next_step(
                top_task_id,
                test_writer_id,
                pass,
                true,
                test_report,
                messages,
            );
            messages.push(format!(
                "System: Task #{} test-writer audit still found critical blockers at pass {}. Max retries ({}) reached; proceeding to deterministic tests.",
                top_task_id, pass, super::MAX_AUDIT_RETRIES
            ));
        } else {
            workflow.set_status(auditor_id, TaskStatus::NeedsChanges);
            workflow.queue.push_back(WorkerJob {
                top_task_id,
                kind: super::WorkerJobKind::TestWriter {
                    test_writer_id,
                    pass: pass.saturating_add(1),
                    feedback: Some(audit_feedback(transcript, code, success)),
                    skip_test_runner_on_success: false,
                    resume_auditor_id: Some(auditor_id),
                    resume_audit_pass: Some(pass.saturating_add(1)),
                },
            });
            messages.push(format!(
                "System: Task #{} test-writer audit requested fixes; test-writer pass {} queued.",
                top_task_id,
                pass.saturating_add(1)
            ));
        }
    } else {
        workflow.set_status(auditor_id, TaskStatus::Done);
        workflow.queue_test_writer_next_step(
            top_task_id,
            test_writer_id,
            pass,
            true,
            test_report,
            messages,
        );
        messages.push(format!(
            "System: Task #{} test-writer audit pass {} complete.",
            top_task_id, pass
        ));
    }
}

fn audit_strictness_policy(pass: u8) -> &'static str {
    match pass {
        1 => {
            "Pass 1 (strict): report all meaningful correctness, safety, reliability, and testability issues."
        }
        2 => {
            "Pass 2 (moderate): prioritize substantial issues and avoid minor nits that do not materially affect behavior."
        }
        3 => "Pass 3 (targeted): focus only on high-impact defects or likely regressions.",
        _ => {
            "Pass 4+ (critical only): only fail for truly critical blockers that would prevent the broader plan from running."
        }
    }
}
