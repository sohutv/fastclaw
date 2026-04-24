use rustyline::completion::Candidate;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum Image {
    Url(url::Url),
    File(PathBuf),
}

impl<'de> Deserialize<'de> for Image {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        if let Ok(url) = url::Url::from_str(&string) {
            Ok(Image::Url(url))
        } else if let Some(path) = PathBuf::from_str(&string).ok().filter(|p| p.exists()) {
            Ok(Image::File(path))
        } else {
            Err(D::Error::custom(format!("unexpected image: {}", string)))
        }
    }
}

impl Serialize for Image {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string = self.to_string();
        serializer.serialize_str(&string)
    }
}

impl Display for Image {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Image::Url(it) => write!(f, "{}", it.display()),
            Image::File(it) => write!(f, "{}", it.display()),
        }
    }
}

impl Image {
    #[allow(unused)]
    pub async fn try_into_image(&self) -> crate::Result<crate::type_::Image> {
        let image = crate::type_::Image::try_from(&self.to_string()).await?;
        Ok(image)
    }
}
