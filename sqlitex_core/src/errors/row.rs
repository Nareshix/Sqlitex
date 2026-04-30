use std::ffi::c_int;

#[derive(thiserror::Error, Debug)]
pub enum RowMapperError {
    #[error("SqliteBusy. Operation took more than 5 seconds")]
    SqliteBusy,

    // in case of any other errors
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },
}
