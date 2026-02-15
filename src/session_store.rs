use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MetaAgentConfig {
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub root_dir: String,
}

impl Default for MetaAgentConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root_dir: "~/.metaagent/sessions".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerTaskFileEntry {
    #[serde(deserialize_with = "deserialize_id_to_string")]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub details: String,
    #[serde(default, deserialize_with = "deserialize_docs_compat")]
    pub docs: Vec<PlannerTaskDocFileEntry>,
    #[serde(default)]
    pub kind: PlannerTaskKindFile,
    #[serde(default)]
    pub status: PlannerTaskStatusFile,
    #[serde(default, deserialize_with = "deserialize_optional_id_to_string")]
    pub parent_id: Option<String>,
    pub order: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerTaskKindFile {
    #[default]
    Task,
    Implementor,
    Auditor,
    TestWriter,
    TestRunner,
    FinalAudit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerTaskStatusFile {
    #[default]
    Pending,
    InProgress,
    NeedsChanges,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerTaskDocFileEntry {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SessionMetadata {
    workspace: String,
    created_at_epoch_secs: u64,
    last_used_epoch_secs: u64,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            workspace: String::new(),
            created_at_epoch_secs: 0,
            last_used_epoch_secs: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListEntry {
    pub session_dir: PathBuf,
    pub workspace: String,
    pub title: Option<String>,
    pub created_at_label: Option<String>,
    pub created_at_epoch_secs: u64,
    pub last_used_epoch_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetaFile {
    pub title: String,
    pub created_at: String,
    #[serde(default)]
    pub stack_description: String,
    #[serde(default)]
    pub test_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskFailFileEntry {
    pub kind: String,
    pub top_task_id: u64,
    pub top_task_title: String,
    pub attempts: u8,
    pub reason: String,
    pub action_taken: String,
    pub created_at_epoch_secs: u64,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    session_dir: PathBuf,
    tasks_file: PathBuf,
    planner_file: PathBuf,
    context_file: PathBuf,
    task_fails_file: PathBuf,
    project_info_file: PathBuf,
    session_meta_file: PathBuf,
    metadata_file: PathBuf,
}

impl SessionStore {
    pub fn initialize(cwd: &Path) -> io::Result<Self> {
        let config = load_config()?;
        let root_dir = expand_home(&config.storage.root_dir)?;
        fs::create_dir_all(&root_dir)?;

        let workspace_name = cwd
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("workspace");
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let session_dir = root_dir.join(format!("{now_secs}-{workspace_name}"));
        fs::create_dir_all(&session_dir)?;

        let store = Self {
            tasks_file: session_dir.join("tasks.json"),
            planner_file: session_dir.join("planner.md"),
            context_file: session_dir.join("rolling_context.json"),
            task_fails_file: session_dir.join("task-fails.json"),
            project_info_file: session_dir.join("project-info.md"),
            session_meta_file: session_dir.join("meta.json"),
            metadata_file: session_dir.join("metadata.json"),
            session_dir,
        };
        store.bootstrap_files(cwd, now_secs)?;
        store.touch_last_used(now_secs)?;
        Ok(store)
    }

    pub fn open_existing(cwd: &Path, session_dir: impl AsRef<Path>) -> io::Result<Self> {
        let session_dir = session_dir.as_ref().to_path_buf();
        fs::create_dir_all(&session_dir)?;
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let store = Self::from_session_dir(session_dir);
        store.bootstrap_files(cwd, now_secs)?;
        store.touch_last_used(now_secs)?;
        Ok(store)
    }

    pub fn list_sessions() -> io::Result<Vec<SessionListEntry>> {
        let config = load_config()?;
        let root_dir = expand_home(&config.storage.root_dir)?;
        fs::create_dir_all(&root_dir)?;
        list_sessions_in_root(&root_dir)
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn tasks_file(&self) -> &Path {
        &self.tasks_file
    }

    pub fn read_tasks(&self) -> io::Result<Vec<PlannerTaskFileEntry>> {
        let text = fs::read_to_string(&self.tasks_file)?;
        let parsed = serde_json::from_str::<Vec<PlannerTaskFileEntry>>(&text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(parsed)
    }

    pub fn planner_file(&self) -> &Path {
        &self.planner_file
    }

    pub fn read_planner_markdown(&self) -> io::Result<String> {
        fs::read_to_string(&self.planner_file)
    }

    pub fn write_rolling_context(&self, entries: &[String]) -> io::Result<()> {
        let text = serde_json::to_string_pretty(entries).map_err(io::Error::other)?;
        fs::write(&self.context_file, text)
    }

    pub fn read_rolling_context(&self) -> io::Result<Vec<String>> {
        let text = fs::read_to_string(&self.context_file)?;
        let parsed = serde_json::from_str::<Vec<String>>(&text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(parsed)
    }

    pub fn task_fails_file(&self) -> &Path {
        &self.task_fails_file
    }

    pub fn read_task_fails(&self) -> io::Result<Vec<TaskFailFileEntry>> {
        let text = fs::read_to_string(&self.task_fails_file)?;
        let parsed = serde_json::from_str::<Vec<TaskFailFileEntry>>(&text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(parsed)
    }

    pub fn append_task_fails(&self, entries: &[TaskFailFileEntry]) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut existing = self.read_task_fails().unwrap_or_default();
        existing.extend_from_slice(entries);
        let text = serde_json::to_string_pretty(&existing).map_err(io::Error::other)?;
        fs::write(&self.task_fails_file, text)
    }

    pub fn project_info_file(&self) -> &Path {
        &self.project_info_file
    }

    pub fn session_meta_file(&self) -> &Path {
        &self.session_meta_file
    }

    pub fn read_project_info(&self) -> io::Result<String> {
        fs::read_to_string(&self.project_info_file)
    }

    pub fn write_project_info(&self, markdown: &str) -> io::Result<()> {
        fs::write(&self.project_info_file, markdown)
    }

    pub fn read_session_meta(&self) -> io::Result<SessionMetaFile> {
        let text = fs::read_to_string(&self.session_meta_file)?;
        serde_json::from_str::<SessionMetaFile>(&text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    fn bootstrap_files(&self, cwd: &Path, now_secs: u64) -> io::Result<()> {
        if !self.tasks_file.exists() {
            fs::write(&self.tasks_file, "[]\n")?;
        }
        if !self.planner_file.exists() {
            fs::write(&self.planner_file, "")?;
        }
        if !self.context_file.exists() {
            fs::write(&self.context_file, "[]\n")?;
        }
        if !self.task_fails_file.exists() {
            fs::write(&self.task_fails_file, "[]\n")?;
        }
        if !self.project_info_file.exists() {
            fs::write(&self.project_info_file, "")?;
        }
        if !self.metadata_file.exists() {
            let metadata = SessionMetadata {
                workspace: cwd.to_string_lossy().to_string(),
                created_at_epoch_secs: now_secs,
                last_used_epoch_secs: now_secs,
            };
            let text = serde_json::to_string_pretty(&metadata).map_err(io::Error::other)?;
            fs::write(&self.metadata_file, text)?;
        }
        Ok(())
    }

    fn from_session_dir(session_dir: PathBuf) -> Self {
        Self {
            tasks_file: session_dir.join("tasks.json"),
            planner_file: session_dir.join("planner.md"),
            context_file: session_dir.join("rolling_context.json"),
            task_fails_file: session_dir.join("task-fails.json"),
            project_info_file: session_dir.join("project-info.md"),
            session_meta_file: session_dir.join("meta.json"),
            metadata_file: session_dir.join("metadata.json"),
            session_dir,
        }
    }

    fn touch_last_used(&self, now_secs: u64) -> io::Result<()> {
        let mut metadata = read_metadata_file(&self.metadata_file).unwrap_or_default();
        if metadata.workspace.is_empty() {
            metadata.workspace = self.session_dir.to_string_lossy().to_string();
        }
        if metadata.created_at_epoch_secs == 0 {
            metadata.created_at_epoch_secs = now_secs;
        }
        metadata.last_used_epoch_secs = now_secs;
        let text = serde_json::to_string_pretty(&metadata).map_err(io::Error::other)?;
        fs::write(&self.metadata_file, text)
    }
}

fn read_metadata_file(path: &Path) -> io::Result<SessionMetadata> {
    let text = fs::read_to_string(path)?;
    let metadata = serde_json::from_str::<SessionMetadata>(&text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(metadata)
}

fn list_sessions_in_root(root_dir: &Path) -> io::Result<Vec<SessionListEntry>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(root_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let store = SessionStore::from_session_dir(path.clone());
        let metadata = read_metadata_file(&store.metadata_file).unwrap_or_default();
        let session_meta = store.read_session_meta().ok();
        let workspace = if metadata.workspace.trim().is_empty() {
            path.display().to_string()
        } else {
            metadata.workspace
        };
        sessions.push(SessionListEntry {
            session_dir: path,
            workspace,
            title: session_meta
                .as_ref()
                .map(|meta| meta.title.trim().to_string())
                .filter(|title| !title.is_empty()),
            created_at_label: session_meta
                .as_ref()
                .map(|meta| meta.created_at.trim().to_string())
                .filter(|value| !value.is_empty()),
            created_at_epoch_secs: metadata.created_at_epoch_secs,
            last_used_epoch_secs: metadata
                .last_used_epoch_secs
                .max(metadata.created_at_epoch_secs),
        });
    }

    sessions.sort_by(|a, b| {
        b.last_used_epoch_secs
            .cmp(&a.last_used_epoch_secs)
            .then_with(|| b.created_at_epoch_secs.cmp(&a.created_at_epoch_secs))
    });
    Ok(sessions)
}

fn load_config() -> io::Result<MetaAgentConfig> {
    let home = home_dir()?;
    let config_dir = home.join(".metaagent");
    fs::create_dir_all(&config_dir)?;
    let config_file = config_dir.join("config.toml");

    if !config_file.exists() {
        let default = "[storage]\nroot_dir = \"~/.metaagent/sessions\"\n";
        fs::write(&config_file, default)?;
    }

    let text = fs::read_to_string(config_file)?;
    let parsed = toml::from_str::<MetaAgentConfig>(&text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(parsed)
}

fn expand_home(raw_path: &str) -> io::Result<PathBuf> {
    if raw_path == "~" {
        return home_dir();
    }
    if let Some(rest) = raw_path.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }
    Ok(PathBuf::from(raw_path))
}

fn home_dir() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IdInput {
    Str(String),
    Num(u64),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DocsInput {
    Entries(Vec<PlannerTaskDocFileEntry>),
    Strings(Vec<String>),
    String(String),
}

fn deserialize_id_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = IdInput::deserialize(deserializer)?;
    Ok(match value {
        IdInput::Str(s) => s,
        IdInput::Num(n) => n.to_string(),
    })
}

fn deserialize_optional_id_to_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<IdInput>::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        IdInput::Str(s) => s,
        IdInput::Num(n) => n.to_string(),
    }))
}

fn deserialize_docs_compat<'de, D>(
    deserializer: D,
) -> Result<Vec<PlannerTaskDocFileEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<DocsInput>::deserialize(deserializer)?;
    Ok(match value {
        None => Vec::new(),
        Some(DocsInput::Entries(entries)) => entries,
        Some(DocsInput::Strings(strings)) => strings_to_docs(strings),
        Some(DocsInput::String(s)) => strings_to_docs(vec![s]),
    })
}

fn strings_to_docs(items: Vec<String>) -> Vec<PlannerTaskDocFileEntry> {
    let mut out = Vec::new();
    for item in items {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains(',') {
            for piece in trimmed.split(',') {
                let piece = piece.trim();
                if piece.is_empty() {
                    continue;
                }
                out.push(PlannerTaskDocFileEntry {
                    title: piece.to_string(),
                    url: piece.to_string(),
                    summary: String::new(),
                });
            }
        } else {
            out.push(PlannerTaskDocFileEntry {
                title: trimmed.to_string(),
                url: trimmed.to_string(),
                summary: String::new(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn expands_home_paths() {
        let expanded = expand_home("~/.metaagent/sessions").expect("home path should expand");
        assert!(expanded.is_absolute());
    }

    #[test]
    fn planner_task_status_defaults_to_pending() {
        let parsed: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task A\",\"parent_id\":null,\"order\":0}",
        )
        .expect("json should parse");
        assert!(parsed.details.is_empty());
        assert!(parsed.docs.is_empty());
        assert!(matches!(parsed.status, PlannerTaskStatusFile::Pending));
        assert!(matches!(parsed.kind, PlannerTaskKindFile::Task));
    }

    #[test]
    fn planner_task_accepts_numeric_or_string_ids() {
        let numeric: PlannerTaskFileEntry =
            serde_json::from_str("{\"id\":1,\"title\":\"Task\",\"parent_id\":2,\"order\":0}")
                .expect("numeric ids should parse");
        assert_eq!(numeric.id, "1");
        assert_eq!(numeric.parent_id.as_deref(), Some("2"));

        let stringy: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task\",\"parent_id\":\"b\",\"order\":0}",
        )
        .expect("string ids should parse");
        assert_eq!(stringy.id, "a");
        assert_eq!(stringy.parent_id.as_deref(), Some("b"));
    }

    #[test]
    fn planner_task_parses_details_field() {
        let parsed: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task\",\"details\":\"More context\",\"parent_id\":null,\"order\":0}",
        )
        .expect("json should parse");
        assert_eq!(parsed.details, "More context");
    }

    #[test]
    fn planner_task_parses_docs_field() {
        let parsed: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task\",\"details\":\"ctx\",\"docs\":[{\"title\":\"Ratatui docs\",\"url\":\"https://docs.rs/ratatui/latest/ratatui/\",\"summary\":\"widgets and rendering\"}],\"parent_id\":null,\"order\":0}",
        )
        .expect("json should parse");
        assert_eq!(parsed.docs.len(), 1);
        assert_eq!(parsed.docs[0].title, "Ratatui docs");
    }

    #[test]
    fn planner_task_parses_legacy_docs_string_field() {
        let parsed: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task\",\"details\":\"ctx\",\"docs\":\"src/app.rs, src/main.rs\",\"parent_id\":null,\"order\":0}",
        )
        .expect("json should parse");
        assert_eq!(parsed.docs.len(), 2);
        assert_eq!(parsed.docs[0].title, "src/app.rs");
        assert_eq!(parsed.docs[1].title, "src/main.rs");
    }

    #[test]
    fn planner_task_parses_empty_legacy_docs_string_as_empty_docs() {
        let parsed: PlannerTaskFileEntry = serde_json::from_str(
            "{\"id\":\"a\",\"title\":\"Task\",\"details\":\"ctx\",\"docs\":\"\",\"parent_id\":null,\"order\":0}",
        )
        .expect("json should parse");
        assert!(parsed.docs.is_empty());
    }

    #[test]
    fn list_sessions_in_root_orders_by_last_used_desc() {
        let base = std::env::temp_dir().join(format!(
            "metaagent-session-list-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should work")
                .as_nanos()
        ));
        fs::create_dir_all(&base).expect("base dir");

        let s1 = base.join("s1");
        let s2 = base.join("s2");
        fs::create_dir_all(&s1).expect("s1");
        fs::create_dir_all(&s2).expect("s2");
        fs::write(
            s1.join("metadata.json"),
            serde_json::to_string_pretty(&SessionMetadata {
                workspace: "/tmp/w1".to_string(),
                created_at_epoch_secs: 10,
                last_used_epoch_secs: 20,
            })
            .expect("serialize"),
        )
        .expect("write s1 metadata");
        fs::write(
            s2.join("metadata.json"),
            serde_json::to_string_pretty(&SessionMetadata {
                workspace: "/tmp/w2".to_string(),
                created_at_epoch_secs: 15,
                last_used_epoch_secs: 30,
            })
            .expect("serialize"),
        )
        .expect("write s2 metadata");
        fs::write(
            s2.join("meta.json"),
            serde_json::to_string_pretty(&SessionMetaFile {
                title: "Planner Session".to_string(),
                created_at: "2026-02-16T12:00:00Z".to_string(),
                stack_description: "Rust + Ratatui terminal UI app".to_string(),
                test_command: Some("cargo test".to_string()),
            })
            .expect("serialize"),
        )
        .expect("write s2 session meta");

        let listed = list_sessions_in_root(&base).expect("list sessions");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].session_dir, s2);
        assert_eq!(listed[1].session_dir, s1);
        assert_eq!(listed[0].title.as_deref(), Some("Planner Session"));
        assert_eq!(
            listed[0].created_at_label.as_deref(),
            Some("2026-02-16T12:00:00Z")
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn open_existing_supports_rolling_context_round_trip() {
        let base = std::env::temp_dir().join(format!(
            "metaagent-session-open-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should work")
                .as_nanos()
        ));
        let session_dir = base.join("session-a");
        fs::create_dir_all(&session_dir).expect("session dir");
        let cwd = std::env::current_dir().expect("cwd");
        let store = SessionStore::open_existing(&cwd, &session_dir).expect("open existing");

        store
            .write_rolling_context(&["one".to_string(), "two".to_string()])
            .expect("write context");
        let read_back = store.read_rolling_context().expect("read context");
        assert_eq!(read_back, vec!["one".to_string(), "two".to_string()]);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn task_fails_round_trip_append() {
        let base = std::env::temp_dir().join(format!(
            "metaagent-session-fails-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should work")
                .as_nanos()
        ));
        let session_dir = base.join("session-a");
        fs::create_dir_all(&session_dir).expect("session dir");
        let cwd = std::env::current_dir().expect("cwd");
        let store = SessionStore::open_existing(&cwd, &session_dir).expect("open existing");

        store
            .append_task_fails(&[TaskFailFileEntry {
                kind: "audit".to_string(),
                top_task_id: 1,
                top_task_title: "Task A".to_string(),
                attempts: 4,
                reason: "Critical blocker".to_string(),
                action_taken: "Continued".to_string(),
                created_at_epoch_secs: 123,
            }])
            .expect("append fails");
        let read_back = store.read_task_fails().expect("read fails");
        assert_eq!(read_back.len(), 1);
        assert_eq!(read_back[0].kind, "audit");
        assert_eq!(read_back[0].top_task_id, 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn session_meta_file_defaults_stack_description_when_missing() {
        let parsed: SessionMetaFile = serde_json::from_str(
            "{\"title\":\"Planner Session\",\"created_at\":\"2026-02-16T12:00:00Z\"}",
        )
        .expect("session meta should parse");
        assert_eq!(parsed.title, "Planner Session");
        assert_eq!(parsed.created_at, "2026-02-16T12:00:00Z");
        assert!(parsed.stack_description.is_empty());
        assert!(parsed.test_command.is_none());
    }

    #[test]
    fn session_meta_file_parses_test_command_as_string_or_null() {
        let with_command: SessionMetaFile = serde_json::from_str(
            "{\"title\":\"Planner Session\",\"created_at\":\"2026-02-16T12:00:00Z\",\"stack_description\":\"Rust\",\"test_command\":\"cargo test\"}",
        )
        .expect("session meta with command should parse");
        assert_eq!(with_command.test_command.as_deref(), Some("cargo test"));

        let without_tests: SessionMetaFile = serde_json::from_str(
            "{\"title\":\"Planner Session\",\"created_at\":\"2026-02-16T12:00:00Z\",\"stack_description\":\"Rust\",\"test_command\":null}",
        )
        .expect("session meta with null command should parse");
        assert!(without_tests.test_command.is_none());
    }
}
