use executors::{executors::BaseCodingAgent, profile::GitCommitAgentConfig};
use git::StatusEntry;

const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const DEFAULT_SYSTEM_PROMPT: &str = "You are a git commit message generator. Given a summary of code changes and a list of \
     modified files, generate a concise, well-structured git commit message. \
     Format: first line is a short imperative title under 72 characters, followed by a blank \
     line, followed by a brief description of the changes if helpful. \
     Output ONLY the commit message text, nothing else.";

/// Maximum number of changed files to include in the prompt
const MAX_FILES_IN_PROMPT: usize = 50;

/// Generate a commit message via a direct one-shot Anthropic API call.
///
/// Returns `None` if the executor is not Claude-based, the API key is missing,
/// or the API call fails — callers should fall back to the raw agent summary.
pub async fn generate_commit_message(
    config: &GitCommitAgentConfig,
    summary: &str,
    changed_files: &[StatusEntry],
) -> Option<String> {
    if !is_claude_based(&config.executor_config.executor) {
        tracing::debug!(
            "Git commit agent executor {:?} is not Claude-based, skipping",
            config.executor_config.executor
        );
        return None;
    }

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            tracing::warn!("Git commit agent: ANTHROPIC_API_KEY not set, skipping");
            return None;
        }
    };

    let model_id = resolve_model_id(config);

    let system_prompt = config
        .system_prompt
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let user_content = build_user_content(summary, changed_files);

    call_anthropic_api(&api_key, &model_id, system_prompt, &user_content).await
}

fn is_claude_based(executor: &BaseCodingAgent) -> bool {
    matches!(
        executor,
        BaseCodingAgent::ClaudeCode | BaseCodingAgent::DockerSandbox
    )
}

fn resolve_model_id(config: &GitCommitAgentConfig) -> String {
    config
        .executor_config
        .model_id
        .as_deref()
        .map(|m| m.trim_start_matches("anthropic/").to_string())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn build_user_content(summary: &str, changed_files: &[StatusEntry]) -> String {
    let files_section = if changed_files.is_empty() {
        String::new()
    } else {
        let file_list: Vec<String> = changed_files
            .iter()
            .take(MAX_FILES_IN_PROMPT)
            .map(|e| {
                let path = String::from_utf8_lossy(&e.path).to_string();
                let status = if e.is_untracked {
                    "A"
                } else if e.staged != ' ' && e.staged != '?' {
                    match e.staged {
                        'D' => "D",
                        'R' => "R",
                        _ => "M",
                    }
                } else {
                    "M"
                };
                format!("  {} {}", status, path)
            })
            .collect();

        let truncated = if changed_files.len() > MAX_FILES_IN_PROMPT {
            format!(
                "\n  ... and {} more files",
                changed_files.len() - MAX_FILES_IN_PROMPT
            )
        } else {
            String::new()
        };

        format!("\n\nChanged files:\n{}{}", file_list.join("\n"), truncated)
    };

    format!("Agent summary:\n{}{}", summary, files_section)
}

async fn call_anthropic_api(
    api_key: &str,
    model_id: &str,
    system_prompt: &str,
    user_content: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": model_id,
            "max_tokens": 512,
            "system": system_prompt,
            "messages": [{"role": "user", "content": user_content}]
        }))
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Git commit agent API request failed: {}", e);
            return None;
        }
    };

    if !response.status().is_success() {
        tracing::warn!("Git commit agent API returned status {}", response.status());
        return None;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Git commit agent API response parse failed: {}", e);
            return None;
        }
    };

    json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
