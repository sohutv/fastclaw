use derive_more::{Deref, From, FromStr};
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    From,
    FromStr,
    Deref,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Default,
)]
pub struct ModelName(String);
