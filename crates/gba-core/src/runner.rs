//! Agent runner — thin wrapper over `claude-agent-sdk-rs`.
//!
//! The `AgentRunner` accepts a `TaskPreset` and builds `ClaudeAgentOptions`
//! accordingly, then delegates to the SDK's `query()` or `ClaudeClient`.

use std::path::PathBuf;

use claude_agent_sdk_rs::{ClaudeAgentOptions, ClaudeClient, ContentBlock, Message};
use futures::StreamExt;
use serde_json::Value;
use tracing::debug;

use crate::{error::GbaCoreError, preset::TaskPreset};

/// Result of an agent query or message send.
#[derive(Debug, Clone)]
pub struct AgentResult {
    /// Concatenated text from assistant messages.
    pub text: String,
    /// Number of turns reported by the SDK.
    pub turns: u32,
    /// Total cost in USD reported by the SDK.
    pub cost_usd: f64,
}

/// Wraps the Claude Agent SDK, building options from `TaskPreset` values.
#[derive(Debug)]
pub struct AgentRunner {
    /// Working directory for agent queries.
    cwd: PathBuf,
    /// Model to use.
    model: String,
    /// Maximum budget in USD.
    max_budget_usd: f64,
}

impl AgentRunner {
    /// Create a new agent runner with base configuration.
    pub fn new(cwd: PathBuf, model: String, max_budget_usd: f64) -> Self {
        Self {
            cwd,
            model,
            max_budget_usd,
        }
    }

    /// Build `ClaudeAgentOptions` from a `TaskPreset`, rendered system prompt,
    /// and optional cwd override.
    fn build_options(
        &self,
        preset: &TaskPreset,
        system_prompt: &str,
        cwd_override: Option<&PathBuf>,
    ) -> ClaudeAgentOptions {
        let effective_cwd = cwd_override.unwrap_or(&self.cwd).clone();

        if preset.max_turns > 0 {
            ClaudeAgentOptions::builder()
                .model(self.model.clone())
                .max_budget_usd(self.max_budget_usd)
                .permission_mode(preset.permission_mode)
                .system_prompt(system_prompt.to_owned())
                .cwd(effective_cwd)
                .max_turns(preset.max_turns)
                .build()
        } else {
            ClaudeAgentOptions::builder()
                .model(self.model.clone())
                .max_budget_usd(self.max_budget_usd)
                .permission_mode(preset.permission_mode)
                .system_prompt(system_prompt.to_owned())
                .cwd(effective_cwd)
                .build()
        }
    }

    /// Execute a one-shot query and collect the result.
    ///
    /// Builds `ClaudeAgentOptions` from the preset, renders the system prompt
    /// externally (caller passes the rendered string), then calls
    /// `claude_agent_sdk_rs::query()`.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::AgentQuery` if the SDK call fails.
    pub async fn query(
        &self,
        preset: &TaskPreset,
        system_prompt: &str,
        user_prompt: &str,
        cwd_override: Option<&PathBuf>,
    ) -> Result<AgentResult, GbaCoreError> {
        let options = self.build_options(preset, system_prompt, cwd_override);

        debug!(
            permission_mode = ?preset.permission_mode,
            max_turns = preset.max_turns,
            "executing agent query"
        );

        let messages = claude_agent_sdk_rs::query(user_prompt, Some(options))
            .await
            .map_err(|e| GbaCoreError::AgentQuery {
                message: "SDK query failed".to_owned(),
                source: Some(e.into()),
            })?;

        Ok(extract_result(&messages))
    }

    /// Create a `ClaudeClient` for multi-turn conversation.
    ///
    /// The caller is responsible for sending messages and consuming responses.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::AgentQuery` if the client fails to connect.
    pub async fn start_session(
        &self,
        preset: &TaskPreset,
        system_prompt: &str,
        cwd_override: Option<&PathBuf>,
    ) -> Result<ClaudeClient, GbaCoreError> {
        let options = self.build_options(preset, system_prompt, cwd_override);

        debug!(
            permission_mode = ?preset.permission_mode,
            "starting agent session"
        );

        let mut client = ClaudeClient::new(options);
        client
            .connect()
            .await
            .map_err(|e| GbaCoreError::AgentQuery {
                message: "failed to connect ClaudeClient".to_owned(),
                source: Some(e.into()),
            })?;

        Ok(client)
    }

    /// Send a message to an existing `ClaudeClient` session and collect the response.
    ///
    /// # Errors
    ///
    /// Returns `GbaCoreError::AgentQuery` if sending or receiving fails.
    pub async fn send(
        client: &mut ClaudeClient,
        message: &str,
    ) -> Result<AgentResult, GbaCoreError> {
        client
            .query(message)
            .await
            .map_err(|e| GbaCoreError::AgentQuery {
                message: "failed to send message".to_owned(),
                source: Some(e.into()),
            })?;

        let mut text_parts: Vec<String> = Vec::new();
        let mut turns: u32 = 0;
        let mut cost_usd: f64 = 0.0;

        {
            let mut stream = client.receive_response();
            while let Some(result) = stream.next().await {
                match result {
                    Ok(msg) => match msg {
                        Message::Assistant(assistant) => {
                            for block in &assistant.message.content {
                                if let ContentBlock::Text(text) = block {
                                    text_parts.push(text.text.clone());
                                }
                            }
                        }
                        Message::Result(result_msg) => {
                            turns = result_msg.num_turns;
                            cost_usd = result_msg.total_cost_usd.unwrap_or(0.0);
                            break;
                        }
                        _ => {}
                    },
                    Err(e) => {
                        return Err(GbaCoreError::AgentQuery {
                            message: "error receiving response".to_owned(),
                            source: Some(e.into()),
                        });
                    }
                }
            }
        }

        Ok(AgentResult {
            text: text_parts.join(""),
            turns,
            cost_usd,
        })
    }

    /// Return the working directory configured for this runner.
    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    /// Return the model configured for this runner.
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Extract text, turns, and cost from a collected `Vec<Message>`.
fn extract_result(messages: &[Message]) -> AgentResult {
    let mut text_parts: Vec<String> = Vec::new();
    let mut turns: u32 = 0;
    let mut cost_usd: f64 = 0.0;

    for msg in messages {
        match msg {
            Message::Assistant(assistant) => {
                for block in &assistant.message.content {
                    if let ContentBlock::Text(text) = block {
                        text_parts.push(text.text.clone());
                    }
                }
            }
            Message::Result(result) => {
                turns = result.num_turns;
                cost_usd = result.total_cost_usd.unwrap_or(0.0);
            }
            _ => {}
        }
    }

    AgentResult {
        text: text_parts.join(""),
        turns,
        cost_usd,
    }
}

/// Render a prompt template using the `PromptManager`.
///
/// Convenience function that maps `GbaPmError` to `GbaCoreError::PromptRender`.
///
/// # Errors
///
/// Returns `GbaCoreError::PromptRender` if the template is not found or rendering fails.
pub fn render_prompt(
    pm: &gba_pm::PromptManager,
    template_name: &str,
    ctx: &Value,
) -> Result<String, GbaCoreError> {
    pm.render(template_name, ctx)
        .map_err(|e| GbaCoreError::PromptRender {
            template: template_name.to_owned(),
            source: e,
        })
}

#[cfg(test)]
mod tests {
    use claude_agent_sdk_rs::PermissionMode;

    use super::*;
    use crate::preset::PresetKind;

    #[test]
    fn test_should_create_agent_runner() {
        let runner = AgentRunner::new(
            PathBuf::from("/tmp/test"),
            "claude-sonnet-4".to_owned(),
            10.0,
        );
        assert_eq!(runner.cwd(), &PathBuf::from("/tmp/test"));
        assert_eq!(runner.model(), "claude-sonnet-4");
    }

    #[test]
    fn test_should_build_options_from_preset() {
        let runner = AgentRunner::new(
            PathBuf::from("/tmp/test"),
            "claude-sonnet-4".to_owned(),
            10.0,
        );

        let preset = PresetKind::Build.preset();
        let options = runner.build_options(&preset, "You are an engineer.", None);

        assert_eq!(
            options.permission_mode,
            Some(PermissionMode::BypassPermissions)
        );
        assert_eq!(options.max_turns, Some(100));
        assert_eq!(options.cwd, Some(PathBuf::from("/tmp/test")));
        assert_eq!(options.model, Some("claude-sonnet-4".to_owned()));
    }

    #[test]
    fn test_should_use_cwd_override() {
        let runner = AgentRunner::new(
            PathBuf::from("/tmp/default"),
            "claude-sonnet-4".to_owned(),
            10.0,
        );

        let override_path = PathBuf::from("/tmp/override");
        let preset = PresetKind::Review.preset();
        let options = runner.build_options(&preset, "Review prompt", Some(&override_path));

        assert_eq!(options.cwd, Some(PathBuf::from("/tmp/override")));
        assert_eq!(options.permission_mode, Some(PermissionMode::Plan));
    }

    #[test]
    fn test_should_not_set_max_turns_when_zero() {
        let runner = AgentRunner::new(
            PathBuf::from("/tmp/test"),
            "claude-sonnet-4".to_owned(),
            10.0,
        );

        // PlanConversation has max_turns == 0 (unlimited)
        let preset = PresetKind::PlanConversation.preset();
        let options = runner.build_options(&preset, "Plan system", None);

        // When max_turns == 0, we don't set it on the options
        assert!(options.max_turns.is_none());
    }

    #[test]
    fn test_should_extract_result_from_messages() {
        use claude_agent_sdk_rs::{
            AssistantMessage, AssistantMessageInner, ResultMessage, TextBlock,
        };

        let messages = vec![
            Message::Assistant(AssistantMessage {
                message: AssistantMessageInner {
                    content: vec![ContentBlock::Text(TextBlock {
                        text: "Hello ".to_owned(),
                    })],
                    model: None,
                    id: None,
                    stop_reason: None,
                    usage: None,
                    error: None,
                },
                parent_tool_use_id: None,
                session_id: None,
                uuid: None,
            }),
            Message::Assistant(AssistantMessage {
                message: AssistantMessageInner {
                    content: vec![ContentBlock::Text(TextBlock {
                        text: "World".to_owned(),
                    })],
                    model: None,
                    id: None,
                    stop_reason: None,
                    usage: None,
                    error: None,
                },
                parent_tool_use_id: None,
                session_id: None,
                uuid: None,
            }),
            Message::Result(ResultMessage {
                subtype: "query_complete".to_owned(),
                duration_ms: 1000,
                duration_api_ms: 800,
                is_error: false,
                num_turns: 3,
                session_id: "sess-1".to_owned(),
                total_cost_usd: Some(0.42),
                usage: None,
                result: None,
                structured_output: None,
            }),
        ];

        let result = extract_result(&messages);
        assert_eq!(result.text, "Hello World");
        assert_eq!(result.turns, 3);
        assert!((result.cost_usd - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn test_should_render_prompt_helper() {
        let pm = gba_pm::PromptManager::new().unwrap();
        let ctx = serde_json::json!({ "feature_slug": "test-feature" });
        let result = render_prompt(&pm, "plan-start-conversation", &ctx);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test-feature"));
    }

    #[test]
    fn test_should_return_error_for_missing_template() {
        let pm = gba_pm::PromptManager::new().unwrap();
        let result = render_prompt(&pm, "nonexistent", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GbaCoreError::PromptRender { .. }
        ));
    }
}
