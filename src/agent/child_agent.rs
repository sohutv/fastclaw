use crate::channels::a2a_channel::A2AChannel;
use crate::agent::{Agent, DelegatedAgent};
use crate::channels::Channel;
use anyhow::anyhow;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
pub struct ChildAgent {
    delegated: Arc<dyn Agent>,
    a2a: Arc<A2AChannel>,
}

impl ChildAgent {
    pub async fn new(delegated: Arc<dyn Agent>) -> crate::Result<Self> {
        if delegated.id().is_main() {
            Err(anyhow!("not child agent"))
        } else {
            let (a2a, _, _) = A2AChannel::new(&delegated)?.start().await?;
            Ok(Self { delegated, a2a })
        }
    }
}

impl DelegatedAgent for ChildAgent {
    fn delegated(&self) -> &Arc<dyn Agent> {
        &self.delegated
    }
}

impl Deref for ChildAgent {
    type Target = dyn Agent;

    fn deref(&self) -> &Self::Target {
        self.delegated.deref()
    }
}
