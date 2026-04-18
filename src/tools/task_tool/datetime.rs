use crate::tools::task_tool::DATETIME_FORMAT;
use chrono::Local;
use derive_more::{Deref, From};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, From, Deref)]
pub struct Datetime(chrono::DateTime<Local>);

impl FromStr for Datetime {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let dt = chrono::NaiveDateTime::parse_from_str(&s, DATETIME_FORMAT)?
            .and_utc()
            .with_timezone(&Local);
        Ok(Self(dt))
    }
}

impl Display for Datetime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format(DATETIME_FORMAT))
    }
}
