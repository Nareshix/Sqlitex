use std::ffi::c_int;

/// Errors that can occur when attempting to open a database connection.
#[derive(thiserror::Error, Debug)]
pub enum SqliteOpenErrors {
    /// This error occurs when SQLite is unable to allocate memory to hold
    /// the database connection object. Usually means the host device is out of RAM.
    #[error("SQLite is unable to allocate memory to hold the database connection object")]
    ConnectionAllocationFailed,

    /// A general SQLite error returned during connection initialization.
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },

    /// The provided file path contains a null byte, which is invalid for C strings.
    #[error("Path contains a null byte. Make sure that there is no Null byte in the file path")]
    EmbeddedNullInFileName,
}

/// Errors that can occur when preparing a SQL statement.
#[derive(thiserror::Error, Debug)]
pub enum SqlitePrepareErrors {
    /// A general SQLite error returned during statement compilation.
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },
}
