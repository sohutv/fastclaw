use super::Base64;
use anyhow::anyhow;
use base64::Engine;
use derive_more::{Deref, Display, From};
use image::GenericImageView;
use std::fmt::Display;
use std::io::Cursor;
use std::ops::{Deref, Div};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Deref, From)]
pub struct Images(Vec<Image>);

impl Images {
    pub async fn as_base64(&self) -> crate::Result<Vec<Base64>> {
        let mut vec = Vec::with_capacity(self.len());
        for image in self.deref() {
            vec.push(image.as_base64().await?);
        }
        Ok(vec)
    }

    pub async fn try_from<S: Display>(value: &[S]) -> crate::Result<Self> {
        let mut vec = Vec::with_capacity(value.len());
        for s in value {
            vec.push(Image::try_from(s.to_string()).await?);
        }
        Ok(vec.into())
    }

    pub async fn align_size_to<const SIZE: usize>(self) -> crate::Result<Self> {
        let Self(images) = self;
        let mut vec = Vec::with_capacity(images.len());
        for image in images {
            vec.push(image.align_size_to::<SIZE>().await?);
        }
        Ok(vec.into())
    }
}

#[derive(Debug, Clone)]
pub enum Image {
    #[allow(unused)]
    Url {
        url: url::Url,
        format: ImgFormat,
    },
    File {
        path: PathBuf,
        format: ImgFormat,
    },
    Raw {
        bytes: Vec<u8>,
        format: ImgFormat,
    },
}

#[derive(Debug, Clone, Copy, Display)]
pub enum ImgFormat {
    #[display("jpg")]
    Jpg,
    #[display("png")]
    Png,
}

impl Image {
    pub async fn as_base64(&self) -> crate::Result<Base64> {
        match self {
            Image::Url { url, format } => {
                let bytes = reqwest::get(url.as_str()).await?.bytes().await?;
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
            Image::File { path, format } => {
                let bytes = tokio::fs::read(path).await?;
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
            Image::Raw { bytes, format } => {
                let string = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok(format!("data:image/{};base64,{}", format, string).into())
            }
        }
    }

    pub fn from_base64<D: Into<Base64>>(data: D) -> crate::Result<Self> {
        let b64 = data.into();
        let data = b64.data();
        let data = base64::engine::general_purpose::STANDARD.decode(&data)?;
        Self::from_bytes(&data)
    }

    pub async fn data(&self) -> crate::Result<(Vec<u8>, ImgFormat)> {
        match self {
            Image::Url { url, format } => {
                let bytes = reqwest::get(url.as_str()).await?.bytes().await?;
                Ok((bytes.to_vec(), *format))
            }
            Image::File { path, format } => {
                let bytes = tokio::fs::read(path).await?;
                Ok((bytes.to_vec(), *format))
            }
            Image::Raw { bytes, format } => Ok((bytes.to_vec(), *format)),
        }
    }

    pub async fn as_image(&self) -> crate::Result<image::DynamicImage> {
        let (data, _) = self.data().await?;
        let image = image::load_from_memory(&data)?;
        Ok(image)
    }

    #[allow(unused)]
    pub async fn as_png(&self) -> crate::Result<Vec<u8>> {
        let image = self.as_image().await?;
        let (w, h) = image.dimensions();
        let mut data = Vec::with_capacity(3usize * w as usize * h as usize);
        let mut cursor = Cursor::new(&mut data);
        let _ = image.write_to(&mut cursor, image::ImageFormat::Png)?;
        Ok(data)
    }

    pub async fn try_from<S: AsRef<str>>(value: S) -> crate::Result<Self> {
        let str = value.as_ref();
        if str.len() > 4096 {
            if let Ok(image) = Self::from_base64(Base64::from_str(str)?) {
                return Ok(image);
            }
        }
        if let Ok(url) = url::Url::from_str(str) {
            let bytes = reqwest::get(url.as_str()).await?.bytes().await?;
            return Self::from_bytes(&bytes);
        }
        if let Ok(bytes) = tokio::fs::read(str).await {
            return Self::from_bytes(&bytes);
        }
        if let Ok(image) = Self::from_base64(Base64::from_str(str)?) {
            return Ok(image);
        }
        Err(anyhow!("unexpected image data"))
    }

    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> crate::Result<Self> {
        let bytes = bytes.as_ref();
        let image = image::load_from_memory(&bytes)?;
        let image = if let Some(exif) = {
            let mut buffer = Cursor::new(&bytes);
            exif::Reader::new().read_from_container(&mut buffer).ok()
        } {
            if let Some(orientation) = exif
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|field| field.value.get_uint(0))
            {
                match orientation {
                    3 => image.rotate180(),
                    6 => image.rotate90(),
                    8 => image.rotate270(),
                    _ => image,
                }
            } else {
                image
            }
        } else {
            image
        };
        let mut buf = vec![];
        let mut cursor = Cursor::new(&mut buf);
        let _ = image.write_to(&mut cursor, image::ImageFormat::Png)?;
        Ok(Image::Raw {
            bytes: buf,
            format: ImgFormat::Png,
        })
    }

    async fn align_size_to<const SIZE: usize>(self) -> crate::Result<Image> {
        let image = self.as_image().await?;
        let data_len = image.as_bytes().len();
        if data_len > SIZE {
            let ratio = (data_len as f32).div(SIZE as f32);
            let (w, h) = image.dimensions();
            let (nw, nh) = ((w as f32 / ratio) as u32, (h as f32 / ratio) as u32);
            let image = image.resize(nw, nh, image::imageops::Lanczos3);
            let mut data = Vec::with_capacity(data_len);
            let cursor = Cursor::new(&mut data);
            let _ = image.write_to(cursor, image::ImageFormat::Png)?;
            Ok(Image::Raw {
                bytes: data,
                format: ImgFormat::Png,
            })
        } else {
            Ok(self)
        }
    }
}

impl From<ImgFormat> for mime::Mime {
    fn from(value: ImgFormat) -> Self {
        match value {
            ImgFormat::Jpg => mime::IMAGE_JPEG,
            ImgFormat::Png => mime::IMAGE_PNG,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_::Image;

    #[tokio::test]
    async fn test() -> crate::Result<()> {
        let image = Image::try_from("/tmp/example.jpg").await?;
        const SIZE: usize = 10 * 1024 * 1024;
        let image = image.align_size_to::<SIZE>().await?;
        let (data, _) = image.data().await?;
        tokio::fs::write(format!("/tmp/example_{}.png", uuid::Uuid::new_v4()), &data).await?;
        Ok(())
    }
}
