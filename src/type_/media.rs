use derive_more::From;

#[allow(unused)]
#[derive(Debug, Clone, From)]
pub enum Media {
    Text(super::Text),
    Image(super::Image),
}
