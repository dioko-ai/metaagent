pub(crate) fn build_task_check_prompt(
    tasks_file: &str,
    project_info_file: &str,
    session_meta_file: &str,
) -> String {
    format!(
        "You are a task-structure audit sub-agent.\n\
         Review the planner JSON file at: {tasks_file}\n\
         You may also read project context at: {project_info_file}\n\
         You may also read session metadata at: {session_meta_file}\n\
         Requirements:\n\
         - If issues are found, edit this tasks.json directly to fix them.\n\
         - Keep task intent/status/order as stable as possible while fixing structure.\n\
         - Validate task hierarchy and ordering against execution guardrails.\n\
         - Enforce test-task shape deterministically: each test_writer must be a direct child of a top-level task (no nested test_writer groups).\n\
         - Focus especially on implementor/auditor/test-runner and test-writer/test-runner relationships.\n\
         - Enforce special-case sequencing for test bootstrapping:\n\
           If tests are absent/unknown (from project-info Testing Setup and/or meta.json test_command is null/empty) and the plan includes test-writing/execution work, ensure there is a dedicated testing-setup top-level task before dependent work.\n\
           That setup task must include implementor and auditor subtasks.\n\
           The setup implementor details must explicitly include both setting up testing tooling and updating meta.json test_command to the exact bash-runnable command string.\n\
           Do not allow non-setup test_writer/test_runner branches to run before that setup task in top-level task order.\n\
         - Return a concise report with either \"PASS\" or \"FIXED\" on the first line, followed by findings.\n\
         - If fixes were applied, list the specific task ids/titles adjusted.\n\
         Then exit."
    )
}
