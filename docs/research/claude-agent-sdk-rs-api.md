# claude-agent-sdk-rs v0.6.x API Research

> Researched: 2026-05-11
> Crate: `claude-agent-sdk-rs` v0.6.4 (latest as of research date)
> Author: Tyr Chen
> License: MIT
> Repository: https://github.com/tyrchen/claude-agent-sdk-rs
> docs.rs: https://docs.rs/claude-agent-sdk-rs/latest/claude_agent_sdk_rs/
> crates.io: https://crates.io/crates/claude-agent-sdk-rs

## Overview

Rust SDK for interacting with Claude Code CLI, enabling programmatic access to Claude's
capabilities with full bidirectional streaming support. Claims 100% feature parity with the
official Python SDK (`claude-agent-sdk-python`). Approximately 4.5K SLoC.

## Installation

```toml
[dependencies]
claude-agent-sdk-rs = "0.6"
```

Feature flags:
- `http` -- Enables HTTP transport support (requires `reqwest`)
- `tracing-support` -- Enables structured logging with `tracing`

## Module Structure

```
claude_agent_sdk_rs
  ├── types/           # Core type definitions, newtypes, builders
  │   ├── config       # ClaudeAgentOptions, SystemPrompt, PermissionMode, SdkPluginConfig
  │   ├── message      # Message, ContentBlock, TextBlock, ToolUseBlock, etc.
  │   └── ...
  ├── client           # ClaudeClient -- interactive bidirectional client
  ├── mcp              # SdkMcpServer, SdkMcpTool, ToolResult, McpServerConfig
  ├── hooks            # HookEvent, HookCallback, HookInput, HookJsonOutput, HookManager
  ├── permissions      # PermissionResult, PermissionUpdate, CanUseToolCallback
  ├── transport        # Communication layer with Claude Code CLI
  ├── control          # Control protocol handler
  ├── message          # Message parsing
  └── error            # Error types
```

## Key Types

### Configuration: `ClaudeAgentOptions`

Builder-pattern struct for configuring queries and client sessions.

```rust
use claude_agent_sdk_rs::{ClaudeAgentOptions, PermissionMode, SdkPluginConfig};

let options = ClaudeAgentOptions::builder()
    // Model selection
    .model("claude-opus-4")
    .fallback_model("claude-sonnet-4")
    // Cost control
    .max_budget_usd(10.0)
    .max_thinking_tokens(2000)
    // Turn limits
    .max_turns(10)
    // Permissions
    .permission_mode(PermissionMode::Default)
    // Tools
    .tools(["Read", "Write", "Bash"])
    .allowed_tools(vec!["mcp__my-tools__greet".to_string()])
    .disallowed_tools(vec![])
    // System prompt
    .system_prompt("You are a helpful coding assistant")
    // MCP servers
    .mcp_servers(mcp_servers)
    // Hooks
    .hooks(hooks)
    // Plugins
    .plugins(vec![SdkPluginConfig::local("./my-plugin")])
    // Session management
    .continue_conversation(session_id)
    .resume(true)
    .fork_session(true)
    // Working directory
    .cwd("/path/to/project")
    .cli_path("/path/to/claude")
    // Environment and args
    .env(env_map)
    .extra_args(vec!["--debug".to_string()])
    // Advanced
    .max_buffer_size(1024 * 1024)
    .stderr_callback(|line| { /* debug output */ })
    .can_use_tool(permission_callback)
    .include_partial_messages(true)
    .output_format(OutputFormat::StreamJson)
    .agents(agent_configs)
    .setting_sources(sources)
    .sandbox(true)
    .user("user-id")
    .add_dirs(vec!["/extra/dir".to_string()])
    .settings(settings_map)
    .betas(vec!["feature-x".to_string()])
    .permission_prompt_tool_name("permission_tool")
    .build();
```

### `PermissionMode` Enum

```rust
enum PermissionMode {
    Default,            // Standard permission behavior
    AcceptEdits,        // Auto-accept file edits
    Plan,               // Planning mode, no execution
    DontAsk,            // Deny anything not pre-approved
    BypassPermissions,  // Bypass all permission checks
}
```

### `Message` Enum

Represents messages streamed from Claude.

```rust
enum Message {
    Assistant(AssistantMessage),  // Claude's response with content blocks
    System(SystemMessage),        // System metadata messages
    Result(ResultMessage),        // Final result with cost info (total_cost_usd)
    // ... possibly other variants for stream events, rate limits
}
```

Usage pattern:
```rust
match message {
    Message::Assistant(msg) => {
        for block in &msg.message.content {
            match block {
                ContentBlock::Text(text) => println!("{}", text.text),
                ContentBlock::ToolUse(tool) => { /* tool.id, tool.name, tool.input */ }
                ContentBlock::Thinking(t) => { /* t.thinking, t.signature */ }
                ContentBlock::ToolResult(r) => { /* r.tool_use_id, r.content */ }
                _ => {}
            }
        }
    }
    Message::Result(result) => {
        println!("Cost: ${}", result.total_cost_usd);
    }
    Message::System(_) => { /* metadata */ }
    _ => {}
}
```

### `ContentBlock` Enum

```rust
enum ContentBlock {
    Text(TextBlock),           // text.text: String
    ToolUse(ToolUseBlock),     // id, name, input (serde_json::Value)
    ToolResult(ToolResultBlock), // tool_use_id, content, is_error
    Thinking(ThinkingBlock),   // thinking, signature
    // possibly: RedactedThinking, Image, Document, SearchResult
}
```

## API Entry Points

### 1. `query()` -- Collecting One-Shot Query

Executes a query and collects all messages into a `Vec`.

```rust
use claude_agent_sdk_rs::{query, ClaudeAgentOptions, Message, ContentBlock};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = ClaudeAgentOptions::builder()
        .model("sonnet")
        .max_turns(5)
        .build();

    let messages = query("What is 2 + 2?", Some(options)).await?;

    for message in messages {
        if let Message::Assistant(msg) = message {
            for block in &msg.message.content {
                if let ContentBlock::Text(text) = block {
                    println!("{}", text.text);
                }
            }
        }
    }
    Ok(())
}
```

### 2. `query_stream()` -- Streaming One-Shot Query

Returns a stream of messages for O(1) memory processing.

```rust
use claude_agent_sdk_rs::{query_stream, Message, ContentBlock};
use futures::StreamExt;

let mut stream = query_stream("Explain Rust ownership", None).await?;

while let Some(result) = stream.next().await {
    let message = result?;
    match message {
        Message::Assistant(msg) => {
            for block in &msg.message.content {
                if let ContentBlock::Text(text) = block {
                    print!("{}", text.text);
                }
            }
        }
        Message::Result(_) => break,
        _ => {}
    }
}
```

### 3. `ClaudeClient` -- Bidirectional Streaming Client

Stateful, multi-turn conversation interface.

```rust
use claude_agent_sdk_rs::{ClaudeClient, ClaudeAgentOptions, PermissionMode, Message};
use futures::StreamExt;

let options = ClaudeAgentOptions::builder()
    .permission_mode(PermissionMode::BypassPermissions)
    .max_turns(5)
    .build();

let mut client = ClaudeClient::new(options);
client.connect().await?;

// First turn
client.query("What is Rust?").await?;
let mut stream = client.receive_response();
while let Some(result) = stream.next().await {
    match result? {
        Message::Assistant(msg) => { /* process content */ }
        Message::Result(_) => break,
        _ => {}
    }
}

// Follow-up turn (maintains conversation context)
client.query("Tell me more about ownership").await?;
let mut stream = client.receive_response();
while let Some(result) = stream.next().await {
    match result? {
        Message::Assistant(msg) => { /* process */ }
        Message::Result(_) => break,
        _ => {}
    }
}

client.disconnect().await?;
```

Dynamic control mid-conversation:
- Interrupt execution
- Change permission mode
- Switch models

## Hooks System

6 hook types for intercepting and controlling Claude's behavior:

### `HookEvent` Enum

```rust
enum HookEvent {
    PreToolUse,         // Before tool execution (safety gate)
    PostToolUse,        // After tool execution (quality gate)
    UserPromptSubmit,   // When user submits a prompt
    Stop,               // When Claude stops
    SubagentStop,       // When a subagent stops
    PreCompact,         // Before context compaction
}
```

### Hook Setup

```rust
use claude_agent_sdk_rs::{HookManager, HookMatcherBuilder, HookEvent, HookOutput};
use std::collections::HashMap;

// Create a callback-based hook
let hook = HookManager::callback(|event_data, tool_name, _context| async move {
    println!("Tool about to be used: {:?}", tool_name);
    Ok(HookOutput::default())  // allow by default
});

// Create a matcher targeting specific tools (or "*" for all)
let matcher = HookMatcherBuilder::new(Some("*"))
    .add_hook(hook)
    .build();

// Register hooks by event type
let mut hooks = HashMap::new();
hooks.insert(HookEvent::PreToolUse, vec![matcher]);

let options = ClaudeAgentOptions::builder()
    .hooks(hooks)
    .build();
```

### HookOutput Fields

For `PreToolUse`:
- `permissionDecision`: "allow" | "deny" | "ask"
- `permissionDecisionReason`: String explanation
- `updatedInput`: Modified tool input

For `PostToolUse`:
- `additionalContext`: String appended to tool result

Top-level fields:
- `systemMessage`: Inject a system message
- `continue`: Boolean to continue/stop

Priority: deny > ask > allow (if multiple hooks fire).

## Custom Tools (MCP)

In-process MCP servers using the `tool!` macro.

### Using `tool!` Macro

```rust
use claude_agent_sdk_rs::{tool, create_sdk_mcp_server, ToolResult, McpToolResultContent};

let greet_tool = tool!(
    "greet",
    "Greet a person by name",
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Name to greet" }
        },
        "required": ["name"]
    }),
    |input: serde_json::Value| async move {
        let name = input["name"].as_str().unwrap_or("World");
        Ok(ToolResult {
            content: vec![McpToolResultContent::Text {
                text: format!("Hello, {}!", name),
            }],
            is_error: false,
        })
    }
);

let server = create_sdk_mcp_server("my-tools", "1.0.0", vec![greet_tool]);
```

### Using `SdkMcpServer` Builder

```rust
use claude_agent_sdk_rs::{SdkMcpServer, SdkMcpTool, ToolResult};

let server = SdkMcpServer::new("calculator")
    .version("1.0.0")
    .tool(SdkMcpTool::new(
        "add",
        "Add two numbers",
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            }
        }),
        |input| Box::pin(async move {
            let sum = input["a"].as_f64().unwrap_or(0.0)
                    + input["b"].as_f64().unwrap_or(0.0);
            Ok(ToolResult::text(format!("Sum: {}", sum)))
        }),
    ));
```

### Registering MCP Servers

```rust
use claude_agent_sdk_rs::McpServerConfig;
use std::collections::HashMap;

let mut mcp_servers = HashMap::new();
mcp_servers.insert("my-tools".to_string(), McpServerConfig::Sdk(server));

let options = ClaudeAgentOptions::builder()
    .mcp_servers(mcp_servers)
    .allowed_tools(vec!["mcp__my-tools__greet".to_string()])
    .build();
```

Tool naming convention: `mcp__{server-name}__{tool-name}`

## Permission Callbacks

Dynamic permission decisions via `can_use_tool` callback.

```rust
use claude_agent_sdk_rs::{PermissionResult, PermissionResultAllow, PermissionResultDeny};

let options = ClaudeAgentOptions::builder()
    .can_use_tool(|tool_name, input, context| async move {
        if tool_name == "Bash" && input["command"].as_str().unwrap_or("").contains("rm") {
            PermissionResult::Deny(PermissionResultDeny {
                message: "Destructive commands not allowed".to_string(),
                ..Default::default()
            })
        } else {
            PermissionResult::Allow(PermissionResultAllow::default())
        }
    })
    .build();
```

`PermissionResult` variants:
- `Allow` -- with optional `updated_input`, `updated_permissions`
- `Deny` -- with `message`, optional `interrupt` flag
- `Ask` -- escalate to human confirmation, with optional `message`, `updated_input`

## Plugins

```rust
use claude_agent_sdk_rs::SdkPluginConfig;

let options = ClaudeAgentOptions::builder()
    .plugins(vec![SdkPluginConfig::local("./my-plugin")])
    .build();
```

## Debugging

```rust
let options = ClaudeAgentOptions::builder()
    .stderr_callback(|line| {
        eprintln!("[DEBUG] {}", line);
    })
    .extra_args(vec!["--debug".to_string()])
    .build();
```

## Examples (23 included in repository)

| Example | Description |
|---------|-------------|
| `01_hello_world` | Basic query usage |
| `02_limit_tool_use` | Restrict allowed tools |
| `03_monitor_tools` | Monitor tool execution |
| `05_hooks_pretooluse` | PreToolUse hooks |
| `06_bidirectional_client` | Multi-turn conversations with ClaudeClient |
| `07_dynamic_control` | Runtime control (interrupt, switch models) |
| `08_mcp_server_integration` | In-process MCP servers |
| `09_agents` | Custom agents |
| `14_streaming_mode` | Comprehensive streaming patterns |
| `15_hooks_comprehensive` | All hook types |
| `17_fallback_model` | Fallback model configuration |
| `20_query_stream` | Streaming query API |

## Key Re-exports (Public API Surface)

The crate root re-exports the most commonly used types:

```rust
// Functions
pub use query;
pub use query_stream;

// Client
pub use ClaudeClient;

// Configuration
pub use ClaudeAgentOptions;
pub use PermissionMode;
pub use SdkPluginConfig;
pub use SystemPrompt; // or SystemPromptConfig

// Messages
pub use Message;
pub use ContentBlock;

// MCP / Tools
pub use tool; // macro
pub use create_sdk_mcp_server;
pub use SdkMcpServer;
pub use SdkMcpTool;
pub use McpServerConfig;
pub use ToolResult;
pub use McpToolResultContent;

// Hooks
pub use HookEvent;
pub use HookManager;
pub use HookMatcherBuilder;
pub use HookOutput;

// Permissions
pub use PermissionResult;
pub use CanUseToolCallback;
```

## Notes

- This is a community crate, not an official Anthropic SDK.
- The official Anthropic SDKs are Python (`claude-agent-sdk-python`) and TypeScript
  (`claude-agent-sdk-typescript`).
- The crate wraps the Claude Code CLI binary (not the HTTP API directly).
- Requires Claude Code CLI to be installed and accessible.
- All async operations use Tokio runtime.
- Uses `serde_json::Value` for dynamic tool inputs.
