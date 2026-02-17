pub(crate) fn build_master_prompt(
    tasks_file: &str,
    workflow_prompt: &str,
) -> String {
    format!(
        "{}\n\
         Planner storage:\n\
         - Read and update this JSON file directly: {tasks_file}\n\
         - Never modify project workspace/source files directly.\n\
         - You may only edit files in the current meta-agent session directory (including tasks/context artifacts).\n\
         - If user requests more work after existing tasks are completed, append new tasks; do not delete completed task history.\n\
         - After task list updates are ready, tell the user `/start` is ready to run.\n\
         - `/start` always resumes from the last unfinished task.\n\
         - File schema: array of objects with fields id, title, details, docs, kind, status, parent_id, order\n\
         - kind values: task, final_audit, implementor, auditor, test_writer, test_runner\n\
         - `docs` is reserved for `/attach-docs`. Do not populate or modify `docs` in master edits.\n\
         - For new tasks created by master, set `docs` to [] and leave it empty.\n\
         - Every task and sub-task must include a non-empty details field with concrete implementation/audit/test intent.\n\
         - Testing-decision flow before initial planning in a session:\n\
           If project info indicates tests are absent/unknown, ask user whether to set up a testing system first.\n\
           If tests exist, ask whether to write new tests as part of this work.\n\
           If tests exist, also ask whether to enforce existing tests (do not break) or ignore tests entirely.\n\
         - Use task structure based on user testing choices:\n\
           Always include implementor under each top-level task.\n\
           Every implementor must include at least one auditor subtask.\n\
           Implementor scope should avoid test-writing/modification unless tied to a direct implementor test_runner branch for existing-test verification.\n\
           Implementor auditors must not include test-related checks; test concerns belong only to test_runner/test_writer branches.\n\
           Include test_writer branch only when user wants new tests written.\n\
           Every test_writer must be a direct child of the top-level task (no nested/umbrella test_writer grouping tasks).\n\
           If user wants existing tests enforced, include test_runner under implementor so failures report back to implementor.\n\
           If implementor has a test_runner, order it after implementor audit subtasks.\n\
           Every test_writer must include at least one test_runner subtask.\n\
           If user chooses to ignore tests, omit test_writer and implementor test_runner branches.\n\
           Special case when tests are absent/unknown and user wants tests:\n\
           Create a dedicated testing-setup top-level task at the earliest position (before feature work that depends on tests).\n\
           That setup task must include an implementor + auditor flow where implementor sets up the test framework/tooling and updates session meta.json test_command to the exact bash-runnable command string.\n\
           Do not add non-setup test_writer or test_runner branches until after that setup task in task order.\n\
         - If user testing choices are still missing, ask concise questions first and wait; do not write tasks.json yet.\n\
         - Update tasks.json only when task state should change.\n\
         - Conversational answers that do not change task state do not require tasks.json edits.\n\
         - Do not ask the user to start execution until task updates are ready.\n\
         - After updating tasks.json, explain to the user what changed.",
        workflow_prompt
    )
}

pub(crate) fn build_convert_plan_prompt(planner_file: &str, tasks_file: &str) -> String {
    format!(
        "You are the master Codex agent and are now in task mode.\n\
         Convert the current planner markdown into executable tasks.\n\
         Read planner markdown at: {planner_file}\n\
         Update tasks JSON at: {tasks_file}\n\
         Requirements:\n\
         - Convert the current plan into concrete task entries and subtasks suitable for execution.\n\
         - Preserve existing completed task history where possible; append/update pending/in-progress work to reflect the plan.\n\
         - Keep task hierarchy valid for this workflow (implementor/auditor/test structure guardrails still apply).\n\
         - Do not modify docs fields except preserving existing values.\n\
         - Save tasks.json and then provide a concise summary of what changed."
    )
}

pub(crate) fn build_session_intro_if_needed(
    prompt: &str,
    session_dir: &str,
    session_meta_file: &str,
    project_info: Option<&str>,
    intro_needed: &mut bool,
) -> String {
    if !*intro_needed {
        return prompt.to_string();
    }
    *intro_needed = false;
    let mut out = format!(
        "Meta-agent session working directory: {session_dir}\n\
         Session metadata file path: {session_meta_file}\n\
         Use this as the shared project context for this master session.\n\n\
         Hard guardrail:\n\
         - Never modify project workspace files directly.\n\
         - You may only create/update files inside the meta-agent session directory above.\n\
         - For planning state, only edit the session task/context artifacts in that session directory.\n\n\
         ",
    );
    if let Some(info) = project_info && !info.trim().is_empty() {
        out.push_str("Project context (project-info.md):\n");
        out.push_str(info);
        out.push_str("\n\n");
    }
    out.push_str(prompt);
    out
}

pub(crate) fn build_failure_report_prompt(
    task_fails_file: &str,
    failed_this_cycle: &[crate::session_store::TaskFailFileEntry],
    has_test_failure: bool,
) -> String {
    let entries = failed_this_cycle
        .iter()
        .map(|entry| {
            format!(
                "- kind={} task_id={} title=\"{}\" attempts={} reason={} action={}",
                entry.kind,
                entry.top_task_id,
                entry.top_task_title,
                entry.attempts,
                entry.reason,
                entry.action_taken
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut prompt = format!(
        "Internal update from execution engine:\n\
         Retry limits were exhausted for one or more branches.\n\
         The canonical failure log has been appended at: {task_fails_file}\n\
         Newly failed entries this cycle:\n\
         {entries}\n\
         Respond with a short user-facing message summarizing what failed and why.\n\
         Do not emit task operations and do not modify files.\n"
    );
    if has_test_failure {
        prompt.push_str(
            "Also ask the user: tests could not be written/kept for some tasks. \
             Would they like these unresolved items written to TODO.md?\n",
        );
    }
    prompt
}

pub(crate) fn split_audits_command_prompt() -> String {
    "Update tasks.json now by splitting audit tasks into more granular audit tasks mapped per concern.\n\
     Concern examples to map across relevant work: correctness, edge cases, tests/coverage, security, performance, and UX.\n\
     Keep task hierarchy coherent and preserve non-audit task intent/status where possible.\n\
     Do not populate or modify docs fields; docs are reserved for /attach-docs.\n\
     After updating tasks.json, provide a concise user-facing summary."
        .to_string()
}

pub(crate) fn merge_audits_command_prompt() -> String {
    "Update tasks.json now by merging overly granular audit tasks back into a simpler audit structure for each implementation branch.\n\
     Keep task hierarchy coherent and preserve non-audit task intent/status where possible.\n\
     Do not populate or modify docs fields; docs are reserved for /attach-docs.\n\
     After updating tasks.json, provide a concise user-facing summary."
        .to_string()
}

pub(crate) fn split_tests_command_prompt() -> String {
    "Update tasks.json now by splitting test_writer tasks into more granular test tasks mapped per concern.\n\
     Concern examples to map across relevant work: core behavior, edge cases, regression paths, error handling, and integration paths.\n\
     Keep task hierarchy coherent and preserve non-test task intent/status where possible.\n\
     Use flat test-writer structure only: each test_writer must be a direct child of the top-level task.\n\
     Do not create umbrella/nested test_writer parent groups.\n\
     Ensure every test_writer has at least one direct test_runner child.\n\
     Do not populate or modify docs fields; docs are reserved for /attach-docs.\n\
     After updating tasks.json, provide a concise user-facing summary."
        .to_string()
}

pub(crate) fn merge_tests_command_prompt() -> String {
    "Update tasks.json now by merging overly granular test_writer tasks back into a simpler test structure for each implementation branch.\n\
     Keep task hierarchy coherent and preserve non-test task intent/status where possible.\n\
     Do not populate or modify docs fields; docs are reserved for /attach-docs.\n\
     After updating tasks.json, provide a concise user-facing summary."
        .to_string()
}
