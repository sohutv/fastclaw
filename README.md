<img src="./asserts/fastclaw_logo_0_small.png" width="128">

# Fastclaw

English | [中文](./README.zh-cn.md)

Fastclaw is a local terminal AI agent built with Rust. It works through an OpenAI-compatible model interface and supports streaming output, reasoning display, conversation context, and tool calls (such as `shell` and `reload-self`).

## Features

- Async agent runtime based on `tokio` + `rig-core`
- OpenAI-compatible model provider integration
- Terminal interaction channel (CLI)
- Separate streaming for reasoning and final response
- Built-in tools:
  - `shell`: execute commands in the workspace directory
  - `reload-self`: trigger agent self-reload
- One-command initialization for workdir and default prompt templates (`workspace/*.md`)

## Requirements

- Rust stable (must support `edition = 2024`)
- Cargo
- Access to an OpenAI-compatible API service

## Install and Build

```bash
git clone <your-repo-url>
cd fastclaw
cargo build
```

## CLI Commands

```bash
fastclaw <COMMAND>
```

Available subcommands:

- `onboard`: initialize config and workdir
- `start`: start the agent

You can also run via Cargo:

```bash
cargo run -- --help
cargo run -- onboard --help
cargo run -- start --help
```

## Quick Start

### 1. Initialize Configuration

By default, Fastclaw initializes into `~/.fastclaw`:

```bash
cargo run -- onboard init-config
```

Initialize into a custom path:

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home
```

If the target directory already exists, use `--rewrite`:

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home --rewrite
```

### 2. Edit `config.toml`

After initialization, a `config.toml` file is generated. Default example:

```toml
default_model_provider = ""
default_model = ""
show_reasoning = true

[model_providers.custom_model_provider_name]
provider_type = "OpenaiCompatible"
api_key = ""
api_url = "https://api.openai.com/v1"

[model_providers.custom_model_provider_name.models]

[log_config]
level = "Info"

[log_config.logger]
logs_dir = "./logs"
```

At minimum, you should:

- Set `default_model_provider` (for example, `"custom_model_provider_name"`)
- Set `default_model` (to a model name available from your provider)
- Fill in `api_key`
- Add model settings under `[model_providers.<provider>.models]`

Example:

```toml
default_model_provider = "openai"
default_model = "gpt-4.1-mini"
show_reasoning = true

[model_providers.openai]
provider_type = "OpenaiCompatible"
api_key = "sk-xxx"
api_url = "https://api.openai.com/v1"

[model_providers.openai.models.gpt-4.1-mini]
temperature = 0.7
tool = true
reasoning = true
websearch = false
vision = false
audio = false
video = false
document = false
reranker = false
embedding = false
```

### 3. Start the Agent

```bash
cargo run -- start --channel Cli
```

Or specify a custom workdir:

```bash
cargo run -- start --channel Cli --workdir /absolute/path/to/fastclaw-home
```

## CLI Interaction

After startup, type messages at the `>>` prompt to chat.

Built-in console commands:

- `/showreasoning on`: show reasoning output
- `/showreasoning off`: hide reasoning output
- `/compact`: reserved, not implemented yet

After each round, token usage is displayed as:

- `<<Tokens:total↑input↓output>>`

## Generated Directory Layout

`onboard init-config` creates a layout like this:

```text
<workdir>/
  config.toml
  workspace/
    AGENTS.md
    BOOTSTRAP.md
    HEARTBEAT.md
    IDENTITY.md
    MEMORY.md
    SOUL.md
    TOOLS.md
    USER.md
    cron/README.md
    memory/README.md
    sessions/README.md
    skills/README.md
    state/README.md
```

## Logging

Logging is configured under `[log_config]`.

- `level` supports: `Error` / `Warn` / `Info` / `Debug`
- Default `logs_dir = "./logs"` resolves to `~/.fastclaw/logs`

## Development

Common commands:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```

## Notes

- Before first startup, ensure model configuration in `config.toml` is complete, otherwise agent creation may fail.
- `start --workdir` must point to an initialized directory containing `config.toml`.
- Startup fails if the log directory is not writable.
