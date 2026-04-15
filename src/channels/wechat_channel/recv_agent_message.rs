use crate::channels::wechat_channel::WechatChannel;
use crate::channels::{ChannelContext, ChannelMessage};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use wechat_sdk::client::WechatClient;

impl WechatChannel {
    pub async fn recv_agent_message(
        dingtalk: Arc<WechatClient>,
        ctx: &ChannelContext,
        receiver: &mut Receiver<ChannelMessage>,
    ) -> crate::Result<()> {
        todo!()
    }
}
