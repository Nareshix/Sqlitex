use std::ffi::c_int;

/// Errors that can occur while mapping or iterating through rows in a query result.
#[derive(thiserror::Error, Debug)]
pub enum RowMapperError {
    /// The database is busy or locked. The operation timed out.
    #[error("SqliteBusy. Operation took more than 5 seconds")]
    SqliteBusy,

    /// A general SQLite failure encountered while stepping through rows.
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },
}