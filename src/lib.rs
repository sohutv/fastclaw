mod agent;
pub mod model_provider;
mod channels;
pub mod cli;
mod config;
mod memory;
mod skills;
mod tools;

pub type Result<T, E = anyhow::Error> = anyhow::Result<T, E>;


#[macro_export]
macro_rules! btree_map {
    () => {
        {
            std::collections::BTreeMap::new()
        }
    };
    ( $($key:expr => $value:expr),* )=>{
        {
            std::collections::BTreeMap::from_iter([($($key, $value),*)])
        }
    }
}
