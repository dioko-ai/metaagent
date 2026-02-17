use super::Workflow;
use super::{
    extract_changed_files_summary, make_context_summary, TaskStatus, WorkerJob, WorkerJobKind,
};

pub(crate) fn build_prompt(
    workflow: &Workflow,
    top_task_id: u64,
    implementor_id: u64,
    feedback: Option<&str>,
) -> String {
    format!(
        "You are an implementation sub-agent.\n\
         Top-level task: {}\n\
         Implementation subtask: {}\n\
         Implementation details:\n{}\n\
         Rolling task context:\n{}\n\
         {}\n\
         Guardrail: do not create or modify tests unless this task explicitly includes a direct implementor test_runner flow reporting failing existing tests.\n\
         End your response with a structured changed-files summary block using this exact format:\n\
         FILES_CHANGED_BEGIN\n\
         - path/to/file.ext: brief description of what changed\n\
         FILES_CHANGED_END\n\
         Include every file you changed. If no files changed, include a single bullet with reason.\n\
         Provide concise progress updates and finish with what changed.",
        workflow.task_title(top_task_id),
        workflow.node_title(implementor_id, "Implementation"),
        workflow.node_details(implementor_id),
        workflow.context_block(),
        feedback
            .as_ref()
            .map(|f| format!("Audit feedback to address:\n{f}"))
            .unwrap_or_else(|| "No audit feedback yet; implement from task prompt.".to_string())
    )
}

pub(crate) fn on_completion(
    workflow: &mut Workflow,
    top_task_id: u64,
    implementor_id: u64,
    pass: u8,
    resume_auditor_id: Option<u64>,
    resume_audit_pass: Option<u8>,
    transcript: &[String],
    success: bool,
    code: i32,
    messages: &mut Vec<String>,
) {
    workflow.push_context(make_context_summary(
        "Implementor",
        &workflow.task_title(top_task_id),
        transcript,
        success,
    ));

    if success {
        // Mark implementation pass complete before moving into audit. If an audit fails,
        // status is set back to NeedsChanges and implementor retries.
        workflow.set_status(implementor_id, TaskStatus::Done);
        if let Some(auditor_id) = resume_auditor_id {
            workflow.queue.push_back(WorkerJob {
                top_task_id,
                kind: WorkerJobKind::Auditor {
                    implementor_id,
                    auditor_id,
                    pass: resume_audit_pass.unwrap_or(1),
                    implementation_report: Some(transcript.join("\n")),
                    changed_files_summary: Some(extract_changed_files_summary(transcript)),
                },
            });
            messages.push(format!(
                "System: Task #{} implementation pass {} complete; resumed audit queued.",
                top_task_id, pass
            ));
        } else {
            if workflow
                .find_child_kind(implementor_id, super::TaskKind::Auditor)
                .is_none()
            {
                let _ = workflow.find_or_create_child_kind(
                    implementor_id,
                    super::TaskKind::Auditor,
                    "Audit",
                );
            }
            let _ = workflow.queue_next_implementor_audit(
                top_task_id,
                implementor_id,
                pass,
                Some(transcript.join("\n")),
                Some(extract_changed_files_summary(transcript)),
                messages,
            );
        }
    } else {
        workflow.set_status(implementor_id, TaskStatus::NeedsChanges);
        workflow.queue.push_back(WorkerJob {
            top_task_id,
            kind: WorkerJobKind::Implementor {
                implementor_id,
                pass: pass.saturating_add(1),
                feedback: Some(format!("Previous implementor run failed with code {code}.")),
                resume_auditor_id: None,
                resume_audit_pass: None,
            },
        });
        messages.push(format!(
            "System: Task #{} implementation failed (code {}); retry queued.",
            top_task_id, code
        ));
    }
}
