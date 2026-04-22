use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_trait::async_trait;
use futures::stream::BoxStream;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;
use workspace_utils::{
    command_ext::GroupSpawnNoWindowExt, msg_store::MsgStore,
    shell::resolve_executable_path_blocking,
};

use crate::{
    command::CmdOverrides,
    env::ExecutionEnv,
    executor_discovery::ExecutorDiscoveredOptions,
    executors::{
        AppendPrompt, AvailabilityInfo, BaseCodingAgent, ExecutorError, SpawnedChild,
        StandardCodingAgentExecutor,
        claude::{ClaudeLogProcessor, HistoryStrategy},
    },
    logs::{
        stderr_processor::normalize_stderr_logs,
        utils::{EntryIndexProvider, patch},
    },
    model_selector::{AgentInfo, ModelSelectorConfig, PermissionPolicy},
    profile::ExecutorConfig,
};

/// The coding agent to run inside the Docker sandbox
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockerSandboxAgent {
    Claude,
    Codex,
    Gemini,
    Copilot,
    Opencode,
    Shell,
}

impl DockerSandboxAgent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Copilot => "copilot",
            Self::Opencode => "opencode",
            Self::Shell => "shell",
        }
    }

    fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            "copilot" => Some(Self::Copilot),
            "opencode" => Some(Self::Opencode),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }
}

impl Default for DockerSandboxAgent {
    fn default() -> Self {
        Self::Claude
    }
}

/// Network access policy for the Docker sandbox
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxNetworkPolicy {
    Open,
    Balanced,
    LockedDown,
}

impl Default for SandboxNetworkPolicy {
    fn default() -> Self {
        Self::Balanced
    }
}

fn default_branch_mode() -> bool {
    true
}

/// Docker Sandbox executor — runs agents in isolated containers via the `sbx` CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema)]
pub struct DockerSandbox {
    #[serde(default)]
    pub append_prompt: AppendPrompt,

    #[serde(default)]
    #[schemars(
        title = "Agent",
        description = "Coding agent to run inside the sandbox (claude, codex, gemini, copilot, opencode, shell)"
    )]
    pub agent: DockerSandboxAgent,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Template",
        description = "Custom template image (e.g. docker.io/my-org/my-template:v1)"
    )]
    pub template: Option<String>,

    #[serde(default = "default_branch_mode")]
    #[schemars(
        title = "Branch Mode",
        description = "Use git worktrees inside the sandbox instead of direct workspace access"
    )]
    pub branch_mode: bool,

    #[serde(default)]
    #[schemars(
        title = "Network Policy",
        description = "Network access policy for the sandbox"
    )]
    pub network_policy: SandboxNetworkPolicy,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "Docker Size",
        description = "Sandbox volume size: 10g, 25g, 50g, or 100g"
    )]
    pub docker_size: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        title = "Extra Mounts",
        description = "Additional host paths to mount into the sandbox (e.g. /Users/you/.claude/agents). Use full absolute paths — ~ is not expanded."
    )]
    pub extra_mounts: Vec<String>,

    #[serde(flatten)]
    pub cmd: CmdOverrides,
}

impl Default for DockerSandbox {
    fn default() -> Self {
        Self {
            append_prompt: AppendPrompt::default(),
            agent: DockerSandboxAgent::default(),
            template: None,
            branch_mode: true,
            network_policy: SandboxNetworkPolicy::default(),
            docker_size: None,
            extra_mounts: vec![],
            cmd: CmdOverrides::default(),
        }
    }
}

impl DockerSandbox {
    /// Derive a stable, per-workspace sandbox name from the workspace directory path.
    fn derive_sandbox_name(workspace_path: &Path) -> String {
        let sanitize = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .to_lowercase()
        };

        let repo_part = workspace_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize)
            .unwrap_or_else(|| "workspace".to_string());

        let workspace_prefix = workspace_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| sanitize(s).chars().take(8).collect::<String>())
            .unwrap_or_else(|| "00000000".to_string());

        let repo_trimmed = repo_part.trim_matches('-');
        let repo_short = if repo_trimmed.len() > 20 {
            &repo_trimmed[..20]
        } else {
            repo_trimmed
        };

        format!(
            "vk-{}-{}",
            workspace_prefix.trim_matches('-'),
            repo_short.trim_matches('-')
        )
    }

    async fn sandbox_is_listed(sandbox_name: &str) -> bool {
        Command::new("sbx")
            .args(["ls"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(sandbox_name))
            .unwrap_or(false)
    }

    async fn remove_sandbox(sandbox_name: &str) {
        let _ = Command::new("sbx")
            .args(["rm", sandbox_name])
            .output()
            .await;
    }

    /// If `workspace_path` is a git worktree (`.git` is a file), returns the path of the
    /// main repository's `.git` directory so it can be mounted into the sandbox alongside
    /// the worktree.  Without this, the gitdir pointer inside the worktree's `.git` file
    /// references a macOS host path that does not exist inside the Linux container.
    fn find_worktree_main_git_dir(workspace_path: &Path) -> Option<PathBuf> {
        let git_file = workspace_path.join(".git");
        if !git_file.is_file() {
            return None;
        }

        let contents = std::fs::read_to_string(&git_file).ok()?;
        // Format: "gitdir: /abs/path/to/main/repo/.git/worktrees/<name>"
        let gitdir_str = contents
            .lines()
            .find_map(|l| l.strip_prefix("gitdir:"))?
            .trim();

        // Walk up the gitdir path to find the component named ".git"
        Path::new(gitdir_str)
            .ancestors()
            .find(|p| p.file_name() == Some(std::ffi::OsStr::new(".git")))
            .map(|p| p.to_path_buf())
    }

    async fn create_sandbox(
        &self,
        sandbox_name: &str,
        workspace_path: &Path,
    ) -> Result<(), ExecutorError> {
        let mut cmd = Command::new("sbx");
        cmd.args(["create", "--name", sandbox_name]);

        if let Some(template) = &self.template {
            cmd.args(["--template", template.as_str()]);
        }

        if let Some(size) = &self.docker_size {
            cmd.args(["--size", size.as_str()]);
        }

        if self.branch_mode {
            cmd.args(["--branch", "auto"]);
        }

        cmd.args([self.agent.as_str(), workspace_path.to_str().unwrap_or(".")]);

        // Mount the main repo's .git directory so that git operations work inside the
        // container.  The worktree's .git file points to an absolute host path; without
        // this extra mount that path is a dead link in the Linux container.
        if let Some(git_dir) = Self::find_worktree_main_git_dir(workspace_path) {
            if let Some(git_dir_str) = git_dir.to_str() {
                cmd.arg(git_dir_str);
            }
        }

        for mount in &self.extra_mounts {
            cmd.arg(mount);
        }

        let status = cmd.status().await.map_err(ExecutorError::Io)?;
        if status.success() {
            return Ok(());
        }
        Err(ExecutorError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to create Docker sandbox '{sandbox_name}'"),
        )))
    }

    async fn ensure_sandbox_exists(
        &self,
        sandbox_name: &str,
        workspace_path: &Path,
    ) -> Result<(), ExecutorError> {
        if Self::sandbox_is_listed(sandbox_name).await {
            return Ok(());
        }

        if self
            .create_sandbox(sandbox_name, workspace_path)
            .await
            .is_ok()
        {
            return Ok(());
        }

        // Creation failed — likely leftover state from a previous crashed attempt.
        Self::remove_sandbox(sandbox_name).await;
        self.create_sandbox(sandbox_name, workspace_path).await
    }

    /// Build the `sbx exec` command with agent-specific flags and the prompt passed as `-p`.
    /// Uses `sbx exec` (no TTY) instead of `sbx run` so stdout is captured as a pipe.
    async fn run_in_sandbox(
        &self,
        sandbox_name: &str,
        prompt: &str,
        resume_args: &[&str],
        current_dir: &Path,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let combined_prompt = self.append_prompt.combine_prompt(prompt);
        let workdir = current_dir.to_str().unwrap_or(".");

        let mut cmd = Command::new("sbx");
        cmd.args(["exec", "-w", workdir, sandbox_name]);

        match self.agent {
            DockerSandboxAgent::Claude => {
                cmd.args([
                    "claude",
                    "-p",
                    &combined_prompt,
                    "--dangerously-skip-permissions",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                ]);
                cmd.args(resume_args);
            }
            DockerSandboxAgent::Codex => {
                cmd.args(["codex", "--full-auto", "-q"]);
                cmd.args(resume_args);
            }
            DockerSandboxAgent::Gemini => {
                cmd.args(["gemini", "--yolo"]);
                cmd.args(resume_args);
            }
            DockerSandboxAgent::Opencode => {
                cmd.args(["opencode", "--yolo"]);
                cmd.args(resume_args);
            }
            DockerSandboxAgent::Copilot => {
                cmd.args(["copilot", "--allow-all-tools"]);
                cmd.args(resume_args);
            }
            DockerSandboxAgent::Shell => {
                cmd.args(["bash"]);
                cmd.args(resume_args);
            }
        }

        cmd.kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(current_dir);

        env.clone()
            .with_profile(&self.cmd)
            .apply_to_command(&mut cmd);

        let child = cmd.group_spawn_no_window()?;
        Ok(child.into())
    }
}

#[async_trait]
impl StandardCodingAgentExecutor for DockerSandbox {
    fn apply_overrides(&mut self, executor_config: &ExecutorConfig) {
        if let Some(agent_id) = &executor_config.agent_id {
            if let Some(agent) = DockerSandboxAgent::from_str_opt(agent_id) {
                self.agent = agent;
            }
        }
        if let Some(permission_policy) = executor_config.permission_policy.clone() {
            self.network_policy = match permission_policy {
                PermissionPolicy::Auto => SandboxNetworkPolicy::Balanced,
                PermissionPolicy::Supervised | PermissionPolicy::Plan => {
                    SandboxNetworkPolicy::LockedDown
                }
            };
        }
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let sandbox_name = Self::derive_sandbox_name(current_dir);
        self.ensure_sandbox_exists(&sandbox_name, current_dir)
            .await?;
        self.run_in_sandbox(&sandbox_name, prompt, &[], current_dir, env)
            .await
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        _reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let sandbox_name = Self::derive_sandbox_name(current_dir);
        self.run_in_sandbox(
            &sandbox_name,
            prompt,
            &["--resume", session_id],
            current_dir,
            env,
        )
        .await
    }

    fn normalize_logs(
        &self,
        msg_store: Arc<MsgStore>,
        current_dir: &Path,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        match self.agent {
            DockerSandboxAgent::Claude => {
                let entry_index_provider = EntryIndexProvider::start_from(&msg_store);
                let h1 = ClaudeLogProcessor::process_logs(
                    msg_store.clone(),
                    current_dir,
                    entry_index_provider.clone(),
                    HistoryStrategy::AmpResume,
                );
                let h2 = normalize_stderr_logs(msg_store, entry_index_provider);
                vec![h1, h2]
            }
            _ => vec![],
        }
    }

    fn default_mcp_config_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        if resolve_executable_path_blocking("sbx").is_some() {
            AvailabilityInfo::InstallationFound
        } else {
            AvailabilityInfo::NotFound
        }
    }

    fn get_preset_options(&self) -> ExecutorConfig {
        ExecutorConfig {
            executor: BaseCodingAgent::DockerSandbox,
            variant: None,
            model_id: None,
            agent_id: Some(self.agent.as_str().to_string()),
            reasoning_id: None,
            permission_policy: Some(PermissionPolicy::Auto),
        }
    }

    async fn discover_options(
        &self,
        _workdir: Option<&Path>,
        _repo_path: Option<&Path>,
    ) -> Result<BoxStream<'static, json_patch::Patch>, ExecutorError> {
        let options = ExecutorDiscoveredOptions {
            model_selector: ModelSelectorConfig {
                agents: vec![
                    AgentInfo {
                        id: "claude".to_string(),
                        label: "Claude".to_string(),
                        description: None,
                        is_default: true,
                    },
                    AgentInfo {
                        id: "codex".to_string(),
                        label: "Codex".to_string(),
                        description: None,
                        is_default: false,
                    },
                    AgentInfo {
                        id: "gemini".to_string(),
                        label: "Gemini".to_string(),
                        description: None,
                        is_default: false,
                    },
                    AgentInfo {
                        id: "copilot".to_string(),
                        label: "Copilot".to_string(),
                        description: None,
                        is_default: false,
                    },
                    AgentInfo {
                        id: "opencode".to_string(),
                        label: "Opencode".to_string(),
                        description: None,
                        is_default: false,
                    },
                    AgentInfo {
                        id: "shell".to_string(),
                        label: "Shell".to_string(),
                        description: None,
                        is_default: false,
                    },
                ],
                permissions: vec![PermissionPolicy::Auto, PermissionPolicy::Supervised],
                ..Default::default()
            },
            ..Default::default()
        };

        Ok(Box::pin(futures::stream::once(async move {
            patch::executor_discovered_options(options)
        })))
    }
}
