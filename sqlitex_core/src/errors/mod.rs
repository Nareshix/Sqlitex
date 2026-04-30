use std::ffi::c_int;

use crate::errors::{connection::SqlitePrepareErrors, row::RowMapperError, statement::StatementStepErrors};

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
}