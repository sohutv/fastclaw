use crate::agent::AgentSettings;
use crate::cli::CmdRunner;
use crate::config::Config;
use crate::model_provider::{ModelProviderName, ModelProviders, ModelSettings};
use crate::ModelName;
use clap::Args;
use std::collections::BTreeMap;
use std::io::{self, Write};

fn prompt(msg: &str, default: Option<&str>) -> String {
    print!("{}", msg);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();
    if input.is_empty() {
        default.unwrap_or_default().to_string()
    } else {
        input.to_string()
    }
}

fn prompt_bool(msg: &str, default: bool) -> bool {
    let default_str = if default { "Y" } else { "N" };
    loop {
        let input = prompt(msg, Some(default_str)).to_lowercase();
        match input.trim() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => continue,
        }
    }
}

#[derive(Args)]
pub struct Onboard {
    #[arg(long)]
    workdir: Option<std::path::PathBuf>,
}

impl CmdRunner for Onboard {
    async fn run(&self) -> crate::Result<()> {
        let workdir = self
            .workdir
            .as_deref()
            .map(|it| it.to_owned())
            .unwrap_or_else(|| Config::default_workdir());

        if !workdir.exists() {
            println!("Creating workdir at {}", workdir.display());
            tokio::fs::create_dir_all(&workdir).await?;
        }

        println!("=== Fastclaw Onboarding ===");
        println!("First, configure your OpenAI-compatible model provider:");

        // Get model provider config
        let provider_name: ModelProviderName = "openai_compatible".to_string().into();
        let api_key = prompt("Enter OpenAI-compatible API key: ", None);
        let api_url = prompt(
            "Enter OpenAI-compatible API URL (default: https://api.openai.com/v1): ",
            Some("https://api.openai.com/v1"),
        );
        let default_model: ModelName = prompt(
            "Enter default model name (default: gpt-4o): ",
            Some("gpt-4o"),
        )
        .into();

        let openai_compatible = crate::model_provider::openai_compatible::OpenaiCompatible {
            api_key: api_key.into(),
            api_url: api_url.parse()?,
            models: {
                let mut map = crate::btree_map!();
                map.insert(default_model.clone(), ModelSettings::default());
                map
            },
        };

        // Build config
        let mut config = Config::default();
        config.default_model_provider = provider_name.clone();
        config.default_model = default_model;
        config.model_providers.insert(
            provider_name,
            ModelProviders::OpenaiCompatible(openai_compatible),
        );
        config.agent_settings.insert("main".into(), AgentSettings::default());
        config.default_show_reasoning = prompt_bool("Show reasoning by default (Y/n): ", true);

        println!("\nNext, let's configure service providers (all optional):");
        let configure_websearch = prompt_bool("Configure web search (Volcengine) (y/N): ", false);
        if configure_websearch {
            let api_url = prompt("Enter Volcengine web search API URL: ", None);
            let api_key = prompt("Enter Volcengine web search API key: ", None);
            config.websearch = Some(crate::service_provider::WebsearchConfigs::Volcengine(
                crate::service_provider::volcengine::websearch::VolcengineWebsearchConfig {
                    api_url: api_url.parse()?,
                    api_key: api_key.into(),
                },
            ));
        }

        let configure_imagegen = prompt_bool("Configure image generation (Volcengine) (y/N): ", false);
        if configure_imagegen {
            let api_url = prompt("Enter Volcengine image generation API URL: ", None);
            let api_key = prompt("Enter Volcengine image generation API key: ", None);
            let model = prompt("Enter Volcengine image generation model (default: doubao-ai-image-pro-1.6): ", Some("doubao-ai-image-pro-1.6")).into();
            config.imagegen = Some(crate::service_provider::ImageGenConfigs::Volcengine(
                crate::service_provider::volcengine::imagegen::VolcengineImageGenConfig {
                    api_url: api_url.parse()?,
                    api_key: api_key.into(),
                    model,
                },
            ));
        }

        let configure_image_enhancer = prompt_bool("Configure image enhancer (Volcengine) (y/N): ", false);
        if configure_image_enhancer {
            let api_url = prompt("Enter Volcengine image enhancer API URL: ", None);
            let access_key = prompt("Enter Volcengine image enhancer access key: ", None);
            let secret_key = prompt("Enter Volcengine image enhancer secret key: ", None);
            config.image_enhancer = Some(crate::service_provider::ImageEnhancerConfigs::Volcengine(
                crate::service_provider::volcengine::image_enhancer::VolcengineImageEnhancerConfig {
                    api_url: api_url.parse()?,
                    access_key: access_key.into(),
                    secret_key: secret_key.into(),
                },
            ));
        }

        let configure_storage = prompt_bool("Configure storage (Volcengine TOS) (y/N): ", false);
        if configure_storage {
            let endpoint = prompt("Enter Volcengine TOS endpoint URL: ", None);
            let region = prompt("Enter Volcengine TOS region (default: cn-north-1): ", Some("cn-north-1"));
            let bucket = prompt("Enter Volcengine TOS bucket name: ", None);
            let access_key = prompt("Enter Volcengine TOS access key: ", None);
            let secret_key = prompt("Enter Volcengine TOS secret key: ", None);
            let key_prefix = prompt("Enter Volcengine TOS key prefix (optional, leave empty for none): ", Some(""));
            config.storage = Some(crate::service_provider::StorageConfigs::Volcengine(
                crate::service_provider::volcengine::storage::VolcengineStorageConfig {
                    endpoint: endpoint.parse()?,
                    region,
                    bucket,
                    access_key: access_key.into(),
                    secret_key: secret_key.into(),
                    key_prefix: if key_prefix.is_empty() { None } else { Some(key_prefix) },
                    connection_timeout_ms: 3_000,
                    request_timeout_ms: 10_000,
                    max_retry_count: 3,
                },
            ));
        }

        let configure_embedding = prompt_bool("Configure embedding (Volcengine) (y/N): ", false);
        if configure_embedding {
            let api_url = prompt("Enter Volcengine embedding API URL: ", None);
            let api_key = prompt("Enter Volcengine embedding API key: ", None);
            let model = prompt("Enter Volcengine embedding model (default: doubao-text-embedding-3-large-1024): ", Some("doubao-text-embedding-3-large-1024")).into();
            config.embedding = Some(crate::service_provider::EmbeddingConfigs::Volcengine(
                crate::service_provider::volcengine::embedding::VolcengineEmbeddingConfig {
                    api_url: api_url.parse()?,
                    api_key: api_key.into(),
                    model,
                },
            ));
        }

        println!("\nNext, configure channels (all optional):");
        #[cfg(feature = "channel_dingtalk_channel")]
        {
            let configure_dingtalk = prompt_bool("Configure DingTalk channel (y/N): ", false);
            if configure_dingtalk {
                let app_key = prompt("Enter DingTalk app key: ", None);
                let app_secret = prompt("Enter DingTalk app secret: ", None);
                let allow_session_ids = BTreeMap::new();
                config.dingtalk_config = Some(crate::channels::dingtalk_channel::DingTalkConfig {
                    credential: dingtalk_stream::Credential {
                        client_id: app_key,
                        client_secret: app_secret,
                    },
                    allow_session_ids,
                });
            }
        }

        #[cfg(feature = "channel_wechat_channel")]
        {
            let configure_wechat = prompt_bool("Configure WeChat channel (y/N): ", false);
            if configure_wechat {
                config.wechat_config = Some(crate::channels::wechat_channel::WechatConfig {
                    session_id: crate::channels::SessionId::Master {
                        val: crate::channels::Master("wechat-master".into()),
                        settings: Default::default(),
                    },
                });
            }
        }

        // Save config
        let config_path = workdir.join("config.toml");
        let config_toml = toml::to_string_pretty(&config)?;
        tokio::fs::write(&config_path, config_toml).await?;

        println!(
            "\n✅ Onboarding complete! Configuration saved at {}",
            config_path.display()
        );

        Ok(())
    }
}