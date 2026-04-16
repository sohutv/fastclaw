<img src="./asserts/fastclaw_logo_0_small.png" width="128">

# Fastclaw

English | [中文](./README.zh-cn.md)

Fastclaw is a local AI agent built with Rust. It supports OpenAI-compatible model providers, multi-channel interaction (CLI, DingTalk, and Wechat), streaming output, conversation history, scheduled task execution, and tool calls.

## Features

- Async runtime based on `tokio` + `rig-core`
- OpenAI-compatible model provider integration
- Channels:
  - CLI channel (`--channel Cli`)
  - DingTalk channel (`--channel Dingtalk`, enabled by default feature)
  - Wechat channel (`--channel Wechat`, enabled by default feature)
- Streaming reasoning and final answer output
- Built-in tools:
  - `shell`: execute shell commands in workspace
  - `current-time`: return local time in RFC3339 format
  - `task-list`, `task-create`, `task-detail-get`, `task-update`, `task-del`
  - `websearch` (only when `[websearch]` is configured)
  - `imagegen` (only when `[imagegen]` is configured)
  - `cloud-storage-store`, `cloud-storage-load`, `cloud-storage-del`
    (only when `cloud_storage_tool` feature is enabled and `[storage]` is configured)
- Session history persistence and history compaction support (`/compact`)
- Background heartbeat scheduler for cron tasks
- One-command onboarding for workdir, default config, and workspace templates

## Requirements

- Rust stable with `edition = 2024`
- Cargo
- Access to an OpenAI-compatible API service

## Build

```bash
git clone <your-repo-url>
cd fastclaw
cargo build
```

If you only need CLI and want to avoid chat-platform dependencies:

```bash
cargo build --no-default-features --features "model_provider_openai_compatible,channel_cli_channel,volcengine"
```

Default features:

- `model_provider_openai_compatible`
- `channel_cli_channel`
- `channel_dingtalk_channel`
- `channel_wechat_channel`
- `volcengine`

## CLI Commands

```bash
fastclaw <COMMAND>
```

Available subcommands:

- `onboard init-config`: initialize workdir and default files
- `start`: start agent runtime

Examples:

```bash
cargo run -- --help
cargo run -- onboard init-config --help
cargo run -- start --help
```

## Quick Start

### 1. Initialize Workdir

Default workdir is `~/.fastclaw`:

```bash
cargo run -- onboard init-config
```

Use a custom path:

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home
```

If directory already exists, add `--rewrite`:

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home --rewrite
```

### 2. Edit `config.toml`

A newly generated `config.toml` currently looks like:

```toml
default_model_provider = ""
default_model = ""
default_show_reasoning = true

[agent_settings]

[model_providers]

[log_config]
level = "Info"

[log_config.logger]
logs_dir = "./logs"

[heartbeat_config]
interval = 60
```

Minimal usable example:

```toml
default_model_provider = "openai"
default_model = "gpt-4.1-mini"
default_show_reasoning = true

[model_providers.openai]
provider_type = "OpenaiCompatible"
api_key = "sk-xxx"
api_url = "https://api.openai.com/v1"

[model_providers.openai.models.gpt-4.1-mini]
vision = true
audio = false
video = false
document = false
websearch = false
reasoning = true
tool = true
reranker = false
embedding = false
max_tokens = 65536

[heartbeat_config]
interval = 60

[log_config]
level = "Info"

[log_config.logger]
logs_dir = "./logs"
```

Optional web search config (`volcengine` feature):

```toml
[websearch]
type = "volcengine"
api_url = "https://<your-volcengine-endpoint>"
api_key = "<your-token>"
```

Optional image generation config (`volcengine` feature):

```toml
[imagegen]
type = "volcengine"
api_url = "https://<your-volcengine-imagegen-endpoint>"
api_key = "<your-token>"
model = "<volcengine-image-model>"
```

Optional cloud storage config (`volcengine` + `cloud_storage_tool` feature):

```toml
[storage]
type = "volcengine"
endpoint = "https://tos-cn-beijing.volces.com"
region = "cn-beijing"
bucket = "your-bucket"
access_key = "AK..."
secret_key = "SK..."
key_prefix = "fastclaw" # optional
connection_timeout_ms = 3000
request_timeout_ms = 10000
max_retry_count = 3
```

Optional DingTalk config (when `channel_dingtalk_channel` is enabled):

```toml
[dingtalk_config.credential]
client_id = "..."
client_secret = "..."

[dingtalk_config.allow_session_ids]
"staff_id_or_group_key" = { Master = { val = "owner", settings = {} } }
```

Optional Wechat config (when `channel_wechat_channel` is enabled):

```toml
[wechat_config]
account_id = "o9cq808B3iiWivLs-uzgKSmbwtXI@im.wechat"
session_id = { Master = { val = "your-wechat-user-id", settings = {} } }
```

### 3. Start Agent

```bash
cargo run -- start --channel Cli
```

With custom workdir:

```bash
cargo run -- start --channel Cli --workdir /absolute/path/to/fastclaw-home
```

## CLI Interaction

After startup, enter message at `>>`.

Console commands:

- `/compact --ratio <0.2~1.0>`: compact current session history
- `/showreasoning on|off`: currently parsed but **not implemented** (see Known Issues)

Per-round usage footer:

- `<<Tokens:total↑input↓output>>`

## Generated Layout

`onboard init-config` creates:

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

`db.sqlite` is created on first `start`, not during `onboard init-config`.

## Logging

`[log_config]` supports:

- `level`: `Error` / `Warn` / `Info` / `Debug`
- `logger`:
  - `Stdout`
  - `File { logs_dir = "./logs" }` (relative path resolves to `<default_workdir>/logs`)

## Development

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test --all-features
```

## Known Issues

- `/showreasoning on|off` triggers `unimplemented!()` in current code path.
- In CLI channel, reasoning text is still printed even when `default_show_reasoning = false`; only heading/closing markers are gated.
- `cargo test --all-features` requires `VOLCENGINE_WEBSEARCH_API_URL` and `VOLCENGINE_WEBSEARCH_API_KEY` for `volcengine` websearch test.
- `onboard init-config` currently writes `resources/CRON_TASK.md` into `workspace/USER.md` (overwriting the previous `USER.md` content).

## Notes

- `start --workdir` must point to initialized workdir containing `config.toml`.
- Startup fails if log directory is not writable.
- Model creation fails if `default_model_provider`, `default_model`, or provider model settings are missing.
