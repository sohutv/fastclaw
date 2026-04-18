use crate::tools::task_tool::DATETIME_FORMAT;
use anyhow::anyhow;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum TaskSchedule {
    Cron(String),
    Datetime(chrono::NaiveDateTime),
}

impl FromStr for TaskSchedule {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, DATETIME_FORMAT) {
            return Ok(TaskSchedule::Datetime(dt));
        }
        if let Ok(utc) = dateparser::parse(&value) {
            return Ok(TaskSchedule::Datetime(utc.naive_local()));
        }
        if let Ok(_) = cron::Schedule::from_str(value) {
            return Ok(TaskSchedule::Cron(value.to_string()));
        }
        Err(anyhow!(
            "unexpected value format, which supports standard cron or time with {DATETIME_FORMAT}"
        ))
    }
}

impl Serialize for TaskSchedule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string = self.to_string();
        serializer.serialize_str(&string)
    }
}

impl<'de> Deserialize<'de> for TaskSchedule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        let dst = Self::from_str(&string).map_err(|err| D::Error::custom(format!("{err}")))?;
        Ok(dst)
    }
}

impl Display for TaskSchedule {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskSchedule::Cron(cron) => write!(f, "{}", cron),
            TaskSchedule::Datetime(dt) => {
                write!(f, "{}", dt.format(DATETIME_FORMAT))
            }
        }
    }
}


