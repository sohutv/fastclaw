use crate::agent::{Agent, AgentRequest, AgentRequestContext, AgentRequestPkg};
use crate::channels::ChannelMessage;
use log::error;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

pub async fn apply(
    agent: Arc<dyn Agent>,
    req: AgentRequest,
    addi_system_prompt: Option<String>,
) -> crate::Result<Receiver<crate::Result<ChannelMessage>>> {
    let (rx, tx) = tokio::sync::mpsc::channel(32);
    let task_id = req.id.clone();
    match spawn_agent_task_inner(
        agent,
        req,
        AgentRequestContext {
            channel_message_sender: rx,
            addi_system_prompt,
            tool_filter: Default::default(),
            with_history: true,
        },
    )
    .await
    {
        Ok(_) => {}
        Err(err) => {
            error!(
                "agent task submit  failed, task_id: {}, error: {}",
                task_id, err
            );
        }
    }
    Ok(tx)
}

#[inline(always)]
async fn spawn_agent_task_inner(
    agent: Arc<dyn Agent>,
    req: AgentRequest,
    ctx: AgentRequestContext,
) -> crate::Result<()> {
    let sender = agent.get_channel_sender().await?;
    let (pkg, ack) = AgentRequestPkg::new_with_ack(req, ctx);
    let _ = sender.send(pkg).await?;
    let _ = ack.await?;
    Ok(())
}
