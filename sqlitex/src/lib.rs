#![doc = include_str!("../docs_io_readme.md")]

pub use sqlitex_core::Connection;
pub use sqlitex_core::errors::{Error, Result};
pub use sqlitex_macros::{migrate, query, query_as};

#[doc(hidden)]
pub mod __private {
    pub use serde;
    pub use sqlitex_core::errors::*;
    pub use sqlitex_core::internal_sqlite::*;
    pub use sqlitex_core::query::*;
    pub use sqlitex_core::traits::*;
    pub use sqlitex_core::utility::*;
    pub use sqlitex_core::libsqlite3_sys;
}