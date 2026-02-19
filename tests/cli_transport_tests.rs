use serde_json::Value;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_metaagent-rust"))
        .args(args)
        .output()
        .expect("run cli")
}

fn run_cli_in_home(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_metaagent-rust"))
        .env("HOME", home)
        .args(args)
        .output()
        .expect("run cli")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout utf8")
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr utf8")
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_str(&stdout_text(output)).expect("json output")
}

struct TempDirGuard {
    path: std::path::PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("metaagent-{prefix}-{nanos}-{counter}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn capability_list_human_output_lists_entries() {
    let output = run_cli(&["api", "capability", "list"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    assert!(stdout.contains("Listed "));
    assert!(stdout.contains("app_prompt_preparation"));
    assert!(stdout.contains("session_planner_storage"));
    assert_eq!(stdout.matches("\n  - ").count(), 12);
    assert!(
        !stdout.trim_start().starts_with('{'),
        "human output should not be a JSON envelope"
    );
}

#[test]
fn workflow_validate_tasks_human_output_is_compact_by_default() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tasks_file =
        std::env::temp_dir().join(format!("metaagent-cli-workflow-validate-{now}.json"));
    std::fs::write(
        &tasks_file,
        r#"[{"id":"1","title":"Task 1","details":"details","parent_id":null,"order":0}]"#,
    )
    .expect("write test tasks");

    let tasks_file_arg = tasks_file.to_string_lossy().to_string();
    let output = run_cli(&[
        "api",
        "workflow",
        "validate-tasks",
        "--tasks-file",
        &tasks_file_arg,
    ]);

    let _ = std::fs::remove_file(tasks_file);
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    assert!(stdout.contains("Validated 1 planner tasks"));
    assert!(stdout.contains("count: 1"));
    assert!(!stdout.contains("\"id\": \"1\""));
    assert!(stdout.contains("  - 1: Task 1"));
    assert!(
        !stdout.trim_start().starts_with('{'),
        "human output should not be a JSON envelope"
    );
}

#[test]
fn workflow_validate_tasks_human_output_verbose_outputs_full_payload() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tasks_file = std::env::temp_dir().join(format!(
        "metaagent-cli-workflow-validate-verbose-{now}.json"
    ));
    std::fs::write(
        &tasks_file,
        r#"[{"id":"1","title":"Task 1","details":"details","parent_id":null,"order":0}]"#,
    )
    .expect("write test tasks");

    let tasks_file_arg = tasks_file.to_string_lossy().to_string();
    let output = run_cli(&[
        "--verbose",
        "api",
        "workflow",
        "validate-tasks",
        "--tasks-file",
        &tasks_file_arg,
    ]);

    let _ = std::fs::remove_file(tasks_file);
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    assert!(stdout.contains("Validated 1 planner tasks"));
    assert!(stdout.contains("Data:"));
    assert!(stdout.contains("\"count\": 1"));
    assert!(stdout.contains("\"id\": \"1\""));
}

#[test]
fn capability_list_json_output_has_expected_shape() {
    let output = run_cli(&["--output", "json", "api", "capability", "list"]);

    assert_eq!(output.status.code(), Some(0));
    let body: Value = serde_json::from_str(&stdout_text(&output)).expect("json output");
    assert_eq!(body.get("status").and_then(Value::as_str), Some("ok"));
    assert!(
        body.get("summary")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("Listed "))
    );

    let first = body
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .expect("at least one capability object");
    assert!(first.get("id").is_some());
    assert!(first.get("domain").is_some());
    assert!(first.get("operation").is_some());
    assert!(first.get("request_contract").is_some());
    assert!(first.get("response_contract").is_some());
    assert!(first.get("code_paths").is_some());
    assert!(first.get("notes").is_some());
}

#[test]
fn unknown_argument_returns_invalid_usage_error() {
    let output = run_cli(&["--definitely-unknown-flag"]);

    assert_ne!(output.status.code(), Some(0));
    let stderr = stderr_text(&output);
    assert!(stderr.contains("Unknown argument:"));
}

#[test]
fn missing_required_flag_returns_usage_error() {
    let output = run_cli(&["api", "capability", "get"]);

    assert_ne!(output.status.code(), Some(0));
    let stderr = stderr_text(&output);
    assert!(stderr.contains("--id <ID>"));
}

#[test]
fn invalid_capability_id_json_error_and_exit_code_are_stable() {
    let output = run_cli(&[
        "--output",
        "json",
        "api",
        "capability",
        "get",
        "--id",
        "definitely-not-real",
    ]);

    assert_eq!(output.status.code(), Some(10));
    let body: Value = serde_json::from_str(&stdout_text(&output)).expect("json output");
    assert_eq!(body.get("status").and_then(Value::as_str), Some("err"));
    assert_eq!(
        body.pointer("/error/code").and_then(Value::as_str),
        Some("invalid_request")
    );
    assert!(
        body.pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Unknown capability id"))
    );
}

#[test]
fn invalid_task_graph_maps_to_validation_failed_exit_code_and_error_code() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tasks_file = std::env::temp_dir().join(format!("metaagent-cli-validate-{now}.json"));
    std::fs::write(
        &tasks_file,
        r#"[{"id":"impl","title":"Impl","details":"d","docs":[],"kind":"implementor","status":"pending","parent_id":null,"order":0}]"#,
    )
    .expect("write invalid tasks file");

    let tasks_file_arg = tasks_file.to_string_lossy().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_metaagent-rust"))
        .args([
            "--output",
            "json",
            "api",
            "workflow",
            "validate-tasks",
            "--tasks-file",
            &tasks_file_arg,
        ])
        .output()
        .expect("run cli");

    let _ = std::fs::remove_file(&tasks_file);
    assert_eq!(output.status.code(), Some(11));
    let body: Value = serde_json::from_str(&stdout_text(&output)).expect("json output");
    assert_eq!(
        body.pointer("/error/code").and_then(Value::as_str),
        Some("validation_failed")
    );
}

#[test]
fn malformed_tasks_json_returns_invalid_request_parse_contract() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tasks_file = std::env::temp_dir().join(format!("metaagent-cli-parse-{now}.json"));
    std::fs::write(&tasks_file, "{ definitely not valid json ]")
        .expect("write malformed tasks file");

    let tasks_file_arg = tasks_file.to_string_lossy().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_metaagent-rust"))
        .args([
            "--output",
            "json",
            "api",
            "workflow",
            "validate-tasks",
            "--tasks-file",
            &tasks_file_arg,
        ])
        .output()
        .expect("run cli");

    let _ = std::fs::remove_file(&tasks_file);
    assert_eq!(output.status.code(), Some(10));
    let body: Value = serde_json::from_str(&stdout_text(&output)).expect("json output");
    assert_eq!(
        body.pointer("/error/code").and_then(Value::as_str),
        Some("invalid_request")
    );
}

#[test]
fn session_cli_command_chain_keeps_json_shape_stable_across_operations() {
    let root = TempDirGuard::new("session-chain");
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let workspace_arg = workspace.display().to_string();
    let init = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "init",
            "--cwd",
            workspace_arg.as_str(),
        ],
    );
    assert_eq!(init.status.code(), Some(0));
    let init_body = stdout_json(&init);
    assert_eq!(init_body.get("status").and_then(Value::as_str), Some("ok"));
    let session_dir = init_body
        .pointer("/data/session_dir")
        .and_then(Value::as_str)
        .expect("session dir from init")
        .to_string();

    let open = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "open",
            "--cwd",
            workspace_arg.as_str(),
            "--session-dir",
            session_dir.as_str(),
        ],
    );
    assert_eq!(open.status.code(), Some(0));
    let open_body = stdout_json(&open);
    assert_eq!(
        open_body
            .pointer("/data/session_dir")
            .and_then(Value::as_str),
        Some(session_dir.as_str())
    );

    let list_first = run_cli_in_home(&home, &["--output", "json", "api", "session", "list"]);
    let list_second = run_cli_in_home(&home, &["--output", "json", "api", "session", "list"]);
    assert_eq!(list_first.status.code(), Some(0));
    assert_eq!(list_second.status.code(), Some(0));
    let list_first_body = stdout_json(&list_first);
    let list_second_body = stdout_json(&list_second);
    assert_eq!(
        list_first_body.get("status"),
        list_second_body.get("status")
    );
    let listed_sessions = list_first_body
        .get("data")
        .and_then(Value::as_array)
        .expect("session list payload");
    assert!(listed_sessions.iter().any(|entry| {
        entry.get("session_dir").and_then(Value::as_str) == Some(session_dir.as_str())
    }));

    let read_tasks = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "read-tasks",
            "--cwd",
            workspace_arg.as_str(),
            "--session-dir",
            session_dir.as_str(),
        ],
    );
    assert_eq!(read_tasks.status.code(), Some(0));
    let read_tasks_body = stdout_json(&read_tasks);
    assert_eq!(
        read_tasks_body.get("status").and_then(Value::as_str),
        Some("ok")
    );
    assert_eq!(
        read_tasks_body
            .pointer("/data/tasks")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let read_planner = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "read-planner",
            "--cwd",
            workspace_arg.as_str(),
            "--session-dir",
            session_dir.as_str(),
        ],
    );
    assert_eq!(read_planner.status.code(), Some(0));
    let read_planner_body = stdout_json(&read_planner);
    assert_eq!(
        read_planner_body
            .pointer("/data/markdown")
            .and_then(Value::as_str),
        Some("")
    );
}

#[test]
fn multi_step_planner_and_workflow_cli_commands_are_chainable_and_json_stable() {
    let root = TempDirGuard::new("workflow-chain");
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let workspace_arg = workspace.display().to_string();

    let init = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "init",
            "--cwd",
            workspace_arg.as_str(),
        ],
    );
    assert_eq!(init.status.code(), Some(0));
    let init_body = stdout_json(&init);
    let session_dir = init_body
        .pointer("/data/session_dir")
        .and_then(Value::as_str)
        .expect("session dir")
        .to_string();

    let tasks_file = std::path::PathBuf::from(&session_dir).join("tasks.json");
    std::fs::write(
        &tasks_file,
        r#"
[
  {"id":"100","title":"Ship CLI parity","details":"Top task","docs":[],"kind":"task","status":"pending","parent_id":null,"order":0},
  {"id":"101","title":"Implement command chain","details":"Implementation details","docs":[],"kind":"implementor","status":"pending","parent_id":"100","order":0},
  {"id":"102","title":"Audit implementation","details":"Audit details","docs":[],"kind":"auditor","status":"pending","parent_id":"101","order":0},
  {"id":"103","title":"Write integration tests","details":"Test writer details","docs":[],"kind":"test_writer","status":"pending","parent_id":"100","order":1},
  {"id":"104","title":"Run integration tests","details":"Test runner details","docs":[],"kind":"test_runner","status":"pending","parent_id":"103","order":0}
]
"#,
    )
    .expect("write seeded tasks");
    let tasks_file_arg = tasks_file.display().to_string();

    let read_tasks = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "session",
            "read-tasks",
            "--cwd",
            workspace_arg.as_str(),
            "--session-dir",
            session_dir.as_str(),
        ],
    );
    assert_eq!(read_tasks.status.code(), Some(0));
    let read_tasks_body = stdout_json(&read_tasks);

    let validate = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "workflow",
            "validate-tasks",
            "--tasks-file",
            tasks_file_arg.as_str(),
        ],
    );
    assert_eq!(validate.status.code(), Some(0));
    let validate_body = stdout_json(&validate);
    assert_eq!(
        read_tasks_body.pointer("/data/tasks"),
        validate_body.pointer("/data/tasks")
    );

    let right_pane_first = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "workflow",
            "right-pane-view",
            "--tasks-file",
            tasks_file_arg.as_str(),
            "--width",
            "60",
        ],
    );
    let right_pane_second = run_cli_in_home(
        &home,
        &[
            "--output",
            "json",
            "api",
            "workflow",
            "right-pane-view",
            "--tasks-file",
            tasks_file_arg.as_str(),
            "--width",
            "60",
        ],
    );
    assert_eq!(right_pane_first.status.code(), Some(0));
    assert_eq!(right_pane_second.status.code(), Some(0));
    let pane_first_body = stdout_json(&right_pane_first);
    let pane_second_body = stdout_json(&right_pane_second);
    assert_eq!(
        pane_first_body.get("status"),
        Some(&Value::String("ok".to_string()))
    );
    assert_eq!(pane_first_body.get("data"), pane_second_body.get("data"));
    let lines = pane_first_body
        .pointer("/data/lines")
        .and_then(Value::as_array)
        .expect("right pane lines");
    assert!(lines.iter().any(|line| {
        line.as_str()
            .is_some_and(|text| text.contains("Ship CLI parity"))
    }));
}
