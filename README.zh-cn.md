<img src="./asserts/fastclaw_logo_0_small.png" width="128">

# Fastclaw

[English](./README.en.md) | 中文

Fastclaw 是一个基于 Rust 的本地终端 AI Agent。它通过 OpenAI-Compatible 模型接口工作，支持流式输出、推理内容显示、会话上下文和工具调用（如 shell、reload-self）。

## 功能特性

- 基于 `tokio` + `rig-core` 的异步 Agent 运行时
- OpenAI-Compatible 模型提供商接入
- 终端交互通道（CLI）
- 支持推理流与回答流分离输出
- 内置工具：
  - `shell`：在工作目录执行命令
  - `reload-self`：触发 Agent 自重载
- 一键初始化工作目录与默认提示模板（`workspace/*.md`）

## 环境要求

- Rust 稳定版（需支持 `edition = 2024`）
- Cargo
- 可访问的 OpenAI-Compatible API 服务

## 安装与构建

```bash
git clone <your-repo-url>
cd fastclaw
cargo build
```

## CLI 命令

```bash
fastclaw <COMMAND>
```

可用子命令：

- `onboard`：初始化配置与工作目录
- `start`：启动 Agent

也可直接用 Cargo：

```bash
cargo run -- --help
cargo run -- onboard --help
cargo run -- start --help
```

## 快速开始

### 1. 初始化配置

默认会初始化到 `~/.fastclaw`：

```bash
cargo run -- onboard init-config
```

指定路径初始化：

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home
```

若目标目录已存在，可用 `--rewrite` 覆盖：

```bash
cargo run -- onboard init-config --path /absolute/path/to/fastclaw-home --rewrite
```

### 2. 编辑 `config.toml`

初始化后会生成 `config.toml`。默认内容示例：

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

你至少需要完成：

- 设置 `default_model_provider`（例如 `"custom_model_provider_name"`）
- 设置 `default_model`（例如你可用的模型名）
- 填写 `api_key`
- 在 `[model_providers.<provider>.models]` 下添加对应模型配置

示例：

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

### 3. 启动 Agent

```bash
cargo run -- start --channel Cli
```

或指定工作目录：

```bash
cargo run -- start --channel Cli --workdir /absolute/path/to/fastclaw-home
```

## 交互说明（CLI）

启动后在 `>>` 提示符输入消息即可对话。

内置控制命令：

- `/showreasoning on`：显示推理输出
- `/showreasoning off`：隐藏推理输出
- `/compact`：已预留，当前尚未实现

每轮输出结束后会显示 token 统计：

- `<<Tokens:总量↑输入↓输出>>`

## 初始化后的目录结构

`onboard init-config` 会创建类似结构：

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

## 日志

默认日志级别在 `[log_config]` 配置。

- `level` 支持：`Error` / `Warn` / `Info` / `Debug`
- 默认 `logs_dir = "./logs"` 会被解析到 `~/.fastclaw/logs`

## 开发

常用命令：

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```

## 注意事项

- 首次启动前请确保 `config.toml` 中模型配置完整，否则可能在创建 Agent 时失败。
- `start` 的 `--workdir` 必须是已初始化目录，且包含 `config.toml`。
- 若日志目录不可写，启动阶段会失败。
