use std::collections::HashMap;
use std::io;

use serde::Deserialize;

use crate::agent::{BackendKind, CodexCommandConfig};
use crate::artifact_io::load_merged_metaagent_config_text;
use crate::default_config::DEFAULT_CONFIG_TOML;

pub const DEFAULT_PROFILE_LABEL: &str = "large-smart";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexModelProfile {
    pub model: String,
    pub thinking_effort: Option<String>,
}

impl CodexModelProfile {
    fn from_config(config: CodexModelProfileConfig) -> Option<Self> {
        let model = config.model.trim();
        if model.is_empty() {
            return None;
        }
        let thinking_effort = config
            .thinking_effort
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Some(Self {
            model: model.to_string(),
            thinking_effort,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAgentKind {
    Master,
    MasterReport,
    ProjectInfo,
    DocsAttach,
    TaskCheck,
    WorkerImplementor,
    WorkerAuditor,
    WorkerTestWriter,
    WorkerFinalAudit,
}

#[derive(Debug, Clone)]
pub struct CodexAgentModelRouting {
    profiles: HashMap<String, CodexModelProfile>,
    agent_profiles: AgentProfileAssignments,
    base_command: CodexCommandConfig,
}

impl Default for CodexAgentModelRouting {
    fn default() -> Self {
        Self::from_toml_str("").unwrap_or_else(|_| Self::emergency_fallback())
    }
}

impl CodexAgentModelRouting {
    pub fn load_from_metaagent_config() -> io::Result<Self> {
        let text = load_merged_metaagent_config_text()?;
        Self::from_toml_str(&text)
    }

    pub fn from_toml_str(text: &str) -> io::Result<Self> {
        let default_config = parse_config(DEFAULT_CONFIG_TOML)?;
        let override_config = parse_config(text)?;
        Ok(Self::from_merged_config(default_config, override_config))
    }

    pub fn profile_for(&self, kind: CodexAgentKind) -> CodexModelProfile {
        let label = self.agent_profiles.label_for(kind);
        self.profiles
            .get(label)
            .or_else(|| self.profiles.get(DEFAULT_PROFILE_LABEL))
            .cloned()
            .unwrap_or_else(default_large_smart_profile)
    }

    pub fn base_command_config(&self) -> CodexCommandConfig {
        self.base_command.clone()
    }

    fn from_merged_config(base: MetaAgentConfigFile, override_cfg: MetaAgentConfigFile) -> Self {
        let merged_backend = base.backend.merged_with(override_cfg.backend);
        let merged_codex = base.codex.merged_with(override_cfg.codex);
        Self::from_merged_runtime_config(merged_backend, merged_codex)
    }

    fn from_merged_runtime_config(
        backend: BackendSelectionConfigFile,
        config: CodexModelConfigFile,
    ) -> Self {
        let mut profiles = HashMap::new();
        for (label, profile) in config.model_profiles {
            let Some(parsed_profile) = CodexModelProfile::from_config(profile) else {
                continue;
            };
            profiles.insert(normalize_profile_label(&label), parsed_profile);
        }
        Self {
            profiles,
            agent_profiles: config.agent_profiles.into_runtime(),
            base_command: backend.into_runtime(),
        }
    }

    fn emergency_fallback() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert(
            DEFAULT_PROFILE_LABEL.to_string(),
            default_large_smart_profile(),
        );
        Self {
            profiles,
            agent_profiles: AgentProfileAssignments::default(),
            base_command: CodexCommandConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct MetaAgentConfigFile {
    backend: BackendSelectionConfigFile,
    codex: CodexModelConfigFile,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct BackendSelectionConfigFile {
    selected: Option<String>,
    codex: BackendCommandConfigFile,
    claude: BackendCommandConfigFile,
}

impl BackendSelectionConfigFile {
    fn merged_with(self, override_cfg: Self) -> Self {
        Self {
            selected: override_cfg.selected.or(self.selected),
            codex: self.codex.merged_with(override_cfg.codex),
            claude: self.claude.merged_with(override_cfg.claude),
        }
    }

    fn into_runtime(self) -> CodexCommandConfig {
        let selected = self
            .selected
            .as_deref()
            .map(str::trim)
            .unwrap_or("codex")
            .to_ascii_lowercase();
        let backend_kind = if selected == "claude" {
            BackendKind::Claude
        } else {
            BackendKind::Codex
        };
        let backend_config = match backend_kind {
            BackendKind::Codex => self.codex,
            BackendKind::Claude => self.claude,
        };
        backend_config.into_runtime(backend_kind)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct BackendCommandConfigFile {
    program: Option<String>,
    args_prefix: Option<Vec<String>>,
}

impl BackendCommandConfigFile {
    fn merged_with(self, override_cfg: Self) -> Self {
        Self {
            program: override_cfg.program.or(self.program),
            args_prefix: override_cfg.args_prefix.or(self.args_prefix),
        }
    }

    fn into_runtime(self, backend_kind: BackendKind) -> CodexCommandConfig {
        let mut config = CodexCommandConfig::default_for_backend(backend_kind);
        if let Some(program) = self
            .program
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            config.program = program.to_string();
        }
        if let Some(args_prefix) = self.args_prefix {
            config.args_prefix = args_prefix
                .into_iter()
                .map(|arg| arg.trim().to_string())
                .filter(|arg| !arg.is_empty())
                .collect();
        }
        config
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct CodexModelConfigFile {
    model_profiles: HashMap<String, CodexModelProfileConfig>,
    agent_profiles: AgentProfileAssignmentsConfig,
}

impl CodexModelConfigFile {
    fn merged_with(self, override_cfg: Self) -> Self {
        let mut model_profiles = self.model_profiles;
        for (label, profile) in override_cfg.model_profiles {
            model_profiles.insert(label, profile);
        }
        Self {
            model_profiles,
            agent_profiles: self.agent_profiles.merged_with(override_cfg.agent_profiles),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct CodexModelProfileConfig {
    model: String,
    thinking_effort: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct AgentProfileAssignmentsConfig {
    master: Option<String>,
    master_report: Option<String>,
    project_info: Option<String>,
    docs_attach: Option<String>,
    task_check: Option<String>,
    worker_implementor: Option<String>,
    worker_auditor: Option<String>,
    worker_test_writer: Option<String>,
    worker_final_audit: Option<String>,
}

impl AgentProfileAssignmentsConfig {
    fn merged_with(self, override_cfg: Self) -> Self {
        Self {
            master: override_cfg.master.or(self.master),
            master_report: override_cfg.master_report.or(self.master_report),
            project_info: override_cfg.project_info.or(self.project_info),
            docs_attach: override_cfg.docs_attach.or(self.docs_attach),
            task_check: override_cfg.task_check.or(self.task_check),
            worker_implementor: override_cfg.worker_implementor.or(self.worker_implementor),
            worker_auditor: override_cfg.worker_auditor.or(self.worker_auditor),
            worker_test_writer: override_cfg.worker_test_writer.or(self.worker_test_writer),
            worker_final_audit: override_cfg.worker_final_audit.or(self.worker_final_audit),
        }
    }

    fn into_runtime(self) -> AgentProfileAssignments {
        AgentProfileAssignments {
            master: normalize_assignment(self.master.as_deref()),
            master_report: normalize_assignment(self.master_report.as_deref()),
            project_info: normalize_assignment(self.project_info.as_deref()),
            docs_attach: normalize_assignment(self.docs_attach.as_deref()),
            task_check: normalize_assignment(self.task_check.as_deref()),
            worker_implementor: normalize_assignment(self.worker_implementor.as_deref()),
            worker_auditor: normalize_assignment(self.worker_auditor.as_deref()),
            worker_test_writer: normalize_assignment(self.worker_test_writer.as_deref()),
            worker_final_audit: normalize_assignment(self.worker_final_audit.as_deref()),
        }
    }
}

#[derive(Debug, Clone)]
struct AgentProfileAssignments {
    master: String,
    master_report: String,
    project_info: String,
    docs_attach: String,
    task_check: String,
    worker_implementor: String,
    worker_auditor: String,
    worker_test_writer: String,
    worker_final_audit: String,
}

impl Default for AgentProfileAssignments {
    fn default() -> Self {
        Self {
            master: DEFAULT_PROFILE_LABEL.to_string(),
            master_report: DEFAULT_PROFILE_LABEL.to_string(),
            project_info: DEFAULT_PROFILE_LABEL.to_string(),
            docs_attach: DEFAULT_PROFILE_LABEL.to_string(),
            task_check: DEFAULT_PROFILE_LABEL.to_string(),
            worker_implementor: DEFAULT_PROFILE_LABEL.to_string(),
            worker_auditor: DEFAULT_PROFILE_LABEL.to_string(),
            worker_test_writer: DEFAULT_PROFILE_LABEL.to_string(),
            worker_final_audit: DEFAULT_PROFILE_LABEL.to_string(),
        }
    }
}

impl AgentProfileAssignments {
    fn label_for(&self, kind: CodexAgentKind) -> &str {
        match kind {
            CodexAgentKind::Master => &self.master,
            CodexAgentKind::MasterReport => &self.master_report,
            CodexAgentKind::ProjectInfo => &self.project_info,
            CodexAgentKind::DocsAttach => &self.docs_attach,
            CodexAgentKind::TaskCheck => &self.task_check,
            CodexAgentKind::WorkerImplementor => &self.worker_implementor,
            CodexAgentKind::WorkerAuditor => &self.worker_auditor,
            CodexAgentKind::WorkerTestWriter => &self.worker_test_writer,
            CodexAgentKind::WorkerFinalAudit => &self.worker_final_audit,
        }
    }
}

fn parse_config(text: &str) -> io::Result<MetaAgentConfigFile> {
    toml::from_str::<MetaAgentConfigFile>(text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn normalize_assignment(raw: Option<&str>) -> String {
    let normalized = normalize_profile_label(raw.unwrap_or(DEFAULT_PROFILE_LABEL));
    if normalized.is_empty() {
        return DEFAULT_PROFILE_LABEL.to_string();
    }
    normalized
}

fn normalize_profile_label(label: &str) -> String {
    label.trim().to_ascii_lowercase()
}

fn default_large_smart_profile() -> CodexModelProfile {
    CodexModelProfile {
        model: "gpt-5.3-codex".to_string(),
        thinking_effort: Some("medium".to_string()),
    }
}

#[cfg(test)]
#[path = "../tests/unit/agent_models_tests.rs"]
mod tests;
