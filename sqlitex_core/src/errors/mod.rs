use std::ffi::c_int;

use crate::errors::{
    connection::SqlitePrepareErrors, row::RowMapperError, statement::StatementStepErrors,
};

pub mod connection;
pub mod row;
pub mod statement;

#[derive(thiserror::Error, Debug)]
#[error("SQLite error {code}: {error_msg}")]
pub struct SqliteFailure {
    pub code: c_int,
    pub error_msg: String,
}



#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    /// A previously applied migration file has been modified.
    ///
    /// To protect database integrity, `sqlitex` refuses to boot if a migration
    /// file's checksum has changed after it was already applied.
    #[error(
        "Integrity Error: Migration {version} ({name}) was altered! Expected checksum {expected_checksum}, but found {actual_checksum}."
    )]
    ChecksumMismatch {
        version: i64,
        name: String,
        expected_checksum: i64,
        actual_checksum: i64,
    },

    #[error(
        "Integrity Error: Migration {version} was renamed from '{expected_name}' to '{actual_name}' after being applied!"
    )]
    NameMismatch { version: i64, expected_name: String, actual_name: String },

    #[error(
        "Integrity Error: Migration {version} ({name}) is missing from the directory but was previously applied to the database!"
    )]
    MissingFile { version: i64, name: String },
}


#[derive(thiserror::Error, Debug)]
pub enum SqlWriteError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to execute statement step: {0}")]
    Step(#[from] StatementStepErrors),
}

#[derive(thiserror::Error, Debug)]
pub enum SqlWriteBindingError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to execute statement step: {0}")]
    Step(#[from] StatementStepErrors),

    #[error("Failed to Bind: {0}")]
    Bind(#[from] SqliteFailure),
}

#[derive(thiserror::Error, Debug)]
pub enum SqlReadError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),
}

#[derive(thiserror::Error, Debug)]
pub enum SqlReadErrorBindings {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to Bind: {0}")]
    Bind(#[from] SqliteFailure),
}

/// Unified Error type for transactios since anything can go wrong.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Write(#[from] SqlWriteError),

    #[error(transparent)]
    WriteBinding(#[from] SqlWriteBindingError),

    #[error(transparent)]
    Read(#[from] SqlReadError),

    #[error(transparent)]
    ReadBinding(#[from] SqlReadErrorBindings),

    #[error(transparent)]
    Row(#[from] RowMapperError), // Needed when iterating over results

    #[error(transparent)]
    Db(#[from] SqliteFailure), // Needed for Transaction BEGIN/COMMIT failures

    #[error(transparent)]
    Migration(#[from] MigrationError),
}

impl From<connection::SqlitePrepareErrors> for Error {
    fn from(e: connection::SqlitePrepareErrors) -> Self {
        Error::Read(SqlReadError::Prepare(e))
    }
}