#![deny(missing_docs)]

#[cfg(feature = "macros")]
pub use msim_macros::{main, sim_test, test};

pub use self::config::*;
pub(crate) use self::runtime::context;

pub mod collections;
mod config;
pub mod fs;
mod intercept;
pub mod net;
pub mod plugin;
pub mod rand;
pub mod runtime;
pub mod task;
pub mod time;
mod utils;
