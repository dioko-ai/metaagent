pub(crate) fn build_project_info_prompt(cwd: &str, question: &str, output_path: &str) -> String {
    format!(
        "You are a project-context discovery sub-agent.\n\
         Analyze the repository and gather concise project context for the user question.\n\
         Current working directory: {cwd}\n\
         User question:\n\
         {question}\n\
         Requirements:\n\
         - Inspect only local files in the repository to understand structure, tech stack, and constraints.\n\
         - Do not browse the web, call external tools/services, or include internet-sourced references.\n\
         - Write a concise Markdown brief to this exact path: {output_path}\n\
         - Include sections: \"Project Overview\", \"Language & Tech Stack\", \"File Structure\", \"Relevant Code Areas\", \"Constraints & Conventions\", \"Testing Setup\".\n\
         - In \"Testing Setup\", explicitly state whether tests currently exist, where they are, and the best command to run the project's tests end-to-end.\n\
         - The test command in \"Testing Setup\" must be a single verbatim shell command runnable in bash as-is from the repository root (not a description).\n\
         - If unknown, state unknown and why.\n\
         - Do not propose implementation ideas, plans, or code-level solutions.\n\
         - Focus only on repository lay-of-the-land and concise file/folder summaries that help future agents work quickly without re-scanning the whole project.\n\
         - Do not make unrelated file changes.\n\
         Then output a short completion summary."
    )
}

pub(crate) fn build_session_meta_prompt(user_prompt: &str, output_path: &str) -> String {
    format!(
        "Using the same session context and project info you already gathered, create session metadata.\n\
         Write valid JSON to this exact path: {output_path}\n\
         JSON schema:\n\
         {{\"title\":\"...\",\"created_at\":\"...\",\"stack_description\":\"...\",\"test_command\":\"...\"}}\n\
         Requirements:\n\
         - title: a concise 4-10 word title derived from the user's original request.\n\
         - created_at: current date-time in ISO-8601 UTC format (example: 2026-02-16T20:14:00Z).\n\
         - stack_description: a concise 1-2 sentence description of the project's language/technology stack based on gathered project info.\n\
         - If stack details are uncertain, state that clearly rather than guessing.\n\
         - test_command: the best command to run the project's tests end-to-end.\n\
         - test_command must be one exact command string runnable in bash as-is from the repository root (for example: \"cargo test\", \"go test ./...\", \"npm test\").\n\
         - Do not describe the command or wrap it in markdown/backticks; provide only the raw command string value.\n\
         - If tests are not set up or unknown, set test_command to JSON null.\n\
         - Output file content only as JSON (no markdown).\n\
         - Overwrite the file if it exists.\n\
         Original user request:\n\
         {user_prompt}\n\
         Then output a one-line completion summary."
    )
}
