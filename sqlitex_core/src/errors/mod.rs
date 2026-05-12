use std::ffi::c_int;

use crate::errors::{
    connection::SqlitePrepareErrors, row::RowMapperError, statement::StatementStepErrors,
};

pub mod connection;
pub mod row;
pub mod statement;

/// alias for Result using sqlitex's unified Error type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A generic SQLite failure containing the native C error code and error message.
#[derive(thiserror::Error, Debug)]
#[error("SQLite error {code}: {error_msg}")]
pub struct SqliteFailure {
    /// The SQLite result code.
    pub code: c_int,
    /// The human-readable error message from SQLite.
    pub error_msg: String,
}

/// Errors related to applying database migrations.
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

    /// A previously applied migration file has been renamed.
    #[error(
        "Integrity Error: Migration {version} was renamed from '{expected_name}' to '{actual_name}' after being applied!"
    )]
    NameMismatch { version: i64, expected_name: String, actual_name: String },

    /// A migration file that was previously applied to the database is now missing from the directory.
    #[error(
        "Integrity Error: Migration {version} ({name}) is missing from the directory but was previously applied to the database!"
    )]
    MissingFile { version: i64, name: String },
}

/// Errors that occur during a write operation (e.g., INSERT, UPDATE, DELETE) with no bind parameters.
#[derive(thiserror::Error, Debug)]
pub enum SqlWriteError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to execute statement step: {0}")]
    Step(#[from] StatementStepErrors),
}

/// Errors that occur during a write operation that includes bind parameters.
#[derive(thiserror::Error, Debug)]
pub enum SqlWriteBindingError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to execute statement step: {0}")]
    Step(#[from] StatementStepErrors),

    #[error("Failed to Bind: {0}")]
    Bind(#[from] SqliteFailure),
}

/// Errors that occur during a read operation (e.g., SELECT) with no bind parameters.
#[derive(thiserror::Error, Debug)]
pub enum SqlReadError {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),
}

/// Errors that occur during a read operation that includes bind parameters.
#[derive(thiserror::Error, Debug)]
pub enum SqlReadErrorBindings {
    #[error("Failed to prepare statement: {0}")]
    Prepare(#[from] SqlitePrepareErrors),

    #[error("Failed to Bind: {0}")]
    Bind(#[from] SqliteFailure),
}
/// Unified Error type for transactions and batch operations, covering all possible failure modes.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// A failure occurred when opening or initializing the database connection.
    #[error(transparent)]
    Connection(#[from] connection::SqliteOpenErrors),

    /// A failure occurred during a write statement.
    #[error(transparent)]
    Write(#[from] SqlWriteError),

    /// A failure occurred during a write statement involving bindings.
    #[error(transparent)]
    WriteBinding(#[from] SqlWriteBindingError),

    /// A failure occurred during a read statement.
    #[error(transparent)]
    Read(#[from] SqlReadError),

    /// A failure occurred during a read statement involving bindings.
    #[error(transparent)]
    ReadBinding(#[from] SqlReadErrorBindings),

    /// A failure occurred while mapping or iterating rows.
    #[error(transparent)]
    Row(#[from] RowMapperError),

    /// A database-level failure, typically during Transaction BEGIN/COMMIT operations.
    #[error(transparent)]
    Db(#[from] SqliteFailure),

    /// A failure occurred during the migration process.
    #[error(transparent)]
    Migration(#[from] MigrationError),
}

impl From<connection::SqlitePrepareErrors> for Error {
    fn from(e: connection::SqlitePrepareErrors) -> Self {
        Error::Read(SqlReadError::Prepare(e))
    }
}