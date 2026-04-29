use crate::config::{ApiKey, ApiUrl, Workspace};
use crate::service_provider::{
    DelArgs, DelResult, LoadArgs, LoadResult, Storage, StorageConfig, StoreArgs, StoreResult,
};
use anyhow::anyhow;
use async_trait::async_trait;
use futures_core::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use url::Url;
use ve_tos_rust_sdk::asynchronous::auth::SignerAPI;
use ve_tos_rust_sdk::asynchronous::{
    object::ObjectAPI, object::ObjectContent, tos, tos::AsyncRuntime,
};
use ve_tos_rust_sdk::auth::PreSignedURLInput;
use ve_tos_rust_sdk::credential::{CommonCredentials, CommonCredentialsProvider};
use ve_tos_rust_sdk::enumeration::HttpMethodType;
use ve_tos_rust_sdk::object::{DeleteObjectInput, GetObjectInput, PutObjectFromBufferInput};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolcengineStorageConfig {
    pub endpoint: ApiUrl,
    pub region: String,
    pub bucket: String,
    pub access_key: ApiKey,
    pub secret_key: ApiKey,
    #[serde(default)]
    pub key_prefix: Option<String>,
    #[serde(default = "default_connection_timeout_ms")]
    pub connection_timeout_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    #[serde(default = "default_max_retry_count")]
    pub max_retry_count: u8,
}

const fn default_connection_timeout_ms() -> u64 {
    3_000
}

const fn default_request_timeout_ms() -> u64 {
    10_000
}

const fn default_max_retry_count() -> u8 {
    3
}

type ToClient = tos::TosClientImpl<
    CommonCredentialsProvider<CommonCredentials>,
    CommonCredentials,
    TokioTosRuntime,
>;

#[derive(Clone)]
pub struct VolcengineStorage {
    config: VolcengineStorageConfig,
    tos_client: Arc<ToClient>,
}

#[async_trait]
impl StorageConfig for VolcengineStorageConfig {
    type T = VolcengineStorage;

    async fn try_into_storage(&self) -> crate::Result<Self::T> {
        let tos_client = Arc::new(
            tos::builder::<TokioTosRuntime>()
                .connection_timeout(self.connection_timeout_ms as isize)
                .request_timeout(self.request_timeout_ms as isize)
                .max_retry_count(self.max_retry_count as isize)
                .ak(self.access_key.as_str())
                .sk(self.secret_key.as_str())
                .region(&self.region)
                .endpoint(self.endpoint.as_str())
                .build()
                .map_err(|err| anyhow!("failed to build TOS client, err: {err}"))?,
        );
        Ok(VolcengineStorage {
            config: self.clone(),
            tos_client,
        })
    }
}

#[derive(Default)]
struct TokioTosRuntime;
#[async_trait]
impl AsyncRuntime for TokioTosRuntime {
    type JoinError = tokio::task::JoinError;

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await
    }

    fn spawn<'a, F>(&self, future: F) -> BoxFuture<'a, Result<F::Output, Self::JoinError>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Box::pin(Handle::current().spawn(future))
    }

    fn block_on<F: Future>(&self, future: F) -> F::Output {
        Handle::current().block_on(future)
    }
}

#[async_trait]
impl Storage for VolcengineStorage {
    async fn store(
        &self,
        _workspace: &'static Workspace,
        StoreArgs { key, mime, content }: StoreArgs,
    ) -> crate::Result<StoreResult> {
        let config = self.config.clone();
        let object_key = config.with_prefix(key.as_str());
        let put_input = {
            let bytes = content.into_bytes().await?;
            let mut put_input =
                PutObjectFromBufferInput::new_with_content(&config.bucket, &object_key, &bytes);
            put_input.set_content_length(bytes.len() as i64);
            put_input.set_content_type(mime.to_string());
            put_input
        };
        let put_output = self
            .tos_client
            .put_object_from_buffer(&put_input)
            .await
            .map_err(|err| anyhow!("failed to put object `{object_key}`, err: {err}"))?;

        let download_input = {
            let mut download_input = PreSignedURLInput::new_with_key(config.bucket, &object_key);
            download_input.set_http_method(HttpMethodType::HttpMethodGet);
            download_input.set_expires(Duration::from_hours(7 * 24).as_secs() as i64);
            download_input
        };

        let download_output = self
            .tos_client
            .pre_signed_url(&download_input)
            .await
            .map_err(|err| {
                anyhow!("failed to pre signed url for object `{object_key}`, err: {err}")
            })?;
        Ok(StoreResult {
            key: object_key.into(),
            request_id: put_output.request_id().to_string(),
            signed_url: Url::from_str(download_output.signed_url())?,
        })
    }

    async fn load(
        &self,
        _workspace: &'static Workspace,
        LoadArgs { key }: LoadArgs,
    ) -> crate::Result<LoadResult> {
        let config = self.config.clone();
        let object_key = config.with_prefix(key.as_str());
        let get_input = GetObjectInput::new(&config.bucket, &object_key);
        let mut output = self
            .tos_client
            .get_object(&get_input)
            .await
            .map_err(|err| anyhow!("failed to get object `{object_key}`, err: {err}"))?;
        let content = output
            .read_all()
            .await
            .map_err(|err| anyhow!("failed to read object body `{object_key}`, err: {err}"))?;
        let md5 = format!("{:x}", md5::compute(&content));
        Ok(LoadResult {
            key: object_key.into(),
            content,
            md5,
            request_id: output.request_id().to_string(),
        })
    }

    async fn del(
        &self,
        _workspace: &'static Workspace,
        DelArgs { key }: DelArgs,
    ) -> crate::Result<DelResult> {
        let config = self.config.clone();
        let object_key = config.with_prefix(key.as_str());
        let del_input = DeleteObjectInput::new(&config.bucket, &object_key);
        let output = self
            .tos_client
            .delete_object(&del_input)
            .await
            .map_err(|err| anyhow!("failed to delete object `{object_key}`, err: {err}"))?;
        Ok(DelResult {
            key: object_key.into(),
            request_id: output.request_id().to_string(),
        })
    }
}

impl VolcengineStorageConfig {
    #[allow(unused)]
    fn from_env() -> crate::Result<Self> {
        Ok(Self {
            endpoint: ApiUrl::from_str(std::env::var("VOLCENGINE_TOS_ENDPOINT")?.as_str())?,
            region: std::env::var("VOLCENGINE_TOS_REGION")?,
            bucket: std::env::var("VOLCENGINE_TOS_BUCKET")?,
            access_key: std::env::var("VOLCENGINE_TOS_ACCESS_KEY")?.into(),
            secret_key: std::env::var("VOLCENGINE_TOS_SECRET_KEY")?.into(),
            key_prefix: std::env::var("VOLCENGINE_TOS_KEY_PREFIX").ok(),
            connection_timeout_ms: std::env::var("VOLCENGINE_TOS_CONNECTION_TIMEOUT_MS")
                .ok()
                .and_then(|it| it.parse::<u64>().ok())
                .unwrap_or_else(default_connection_timeout_ms),
            request_timeout_ms: std::env::var("VOLCENGINE_TOS_REQUEST_TIMEOUT_MS")
                .ok()
                .and_then(|it| it.parse::<u64>().ok())
                .unwrap_or_else(default_request_timeout_ms),
            max_retry_count: std::env::var("VOLCENGINE_TOS_MAX_RETRY_COUNT")
                .ok()
                .and_then(|it| it.parse::<u8>().ok())
                .unwrap_or_else(default_max_retry_count),
        })
    }

    fn with_prefix(&self, key: &str) -> String {
        let key = key.trim_start_matches('/');
        match self
            .key_prefix
            .as_deref()
            .map(str::trim)
            .filter(|it| !it.is_empty())
        {
            None => key.to_string(),
            Some(prefix) => {
                let prefix = prefix.trim_matches('/');
                if prefix.is_empty() {
                    key.to_string()
                } else if key.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{prefix}/{key}")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Workspace;
    use crate::service_provider::volcengine::storage::VolcengineStorageConfig;
    use crate::service_provider::{
        Content, DelArgs, LoadArgs, LoadResult, Storage, StorageConfig, StoreArgs, StoreResult,
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn test_storage_workflow() -> crate::Result<()> {
        let config = VolcengineStorageConfig::from_env()?;
        let storage = config.try_into_storage().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));

        let key = format!("downloads/test-{}.txt", Uuid::new_v4());
        let expected = format!("hello fastclaw {}", Uuid::new_v4());
        let StoreResult { signed_url, .. } = storage
            .store(
                workspace,
                StoreArgs {
                    key: key.clone().into(),
                    mime: mime::TEXT_PLAIN,
                    content: Content::String(expected.clone()),
                },
            )
            .await?;

        let loaded = storage.load(workspace, LoadArgs::from(key.clone())).await?;
        assert_eq!(loaded.content, expected.clone().into_bytes());

        dbg!(signed_url.as_str());
        let web_content = reqwest::get(signed_url.as_str()).await?.bytes().await?;
        assert_eq!(web_content, expected.into_bytes());

        let _ = storage.del(workspace, DelArgs::from(key)).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_storage_workflow_img() -> crate::Result<()> {
        let config = VolcengineStorageConfig::from_env()?;
        let storage = config.try_into_storage().await?;
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace::init("/tmp").await?));

        let key = format!("downloads/{}.png", Uuid::new_v4());
        let filepath =workspace.downloads_path.join("example-img.png");
        let expected = tokio::fs::read(&filepath).await?;
        let input_md5 = format!("{:x}", md5::compute(&expected));
        let StoreResult { signed_url, .. } = storage
            .store(
                workspace,
                StoreArgs {
                    key: key.clone().into(),
                    mime: mime::TEXT_PLAIN,
                    content: Content::File(filepath),
                },
            )
            .await?;

        let LoadResult { md5, .. } = storage.load(workspace, LoadArgs::from(key.clone())).await?;

        assert_eq!(&md5, &input_md5);

        dbg!(signed_url.as_str());
        let web_content = reqwest::get(signed_url.as_str()).await?.bytes().await?;
        let md5 = format!("{:x}", md5::compute(&web_content));
        assert_eq!(&md5, &input_md5);

        let _ = storage.del(workspace, DelArgs::from(key)).await?;
        Ok(())
    }
}
