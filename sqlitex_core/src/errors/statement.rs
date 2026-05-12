use std::ffi::c_int;

/// Errors that can occur when stepping (executing) a prepared statement.
#[derive(thiserror::Error, Debug)]
pub enum StatementStepErrors {
    /// The database is busy or locked. The operation timed out.
    #[error("SqliteBusy. Operation took more than 5 seconds")]
    SqliteBusy,

    /// A foreign key constraint was violated during execution.
    #[error("Foreign key constraint failed. Sqlite error {code} : {error_msg}")]
    ForeignKeyConstraint { code: c_int, error_msg: String },

    /// A UNIQUE or PRIMARY KEY constraint was violated during execution.
    #[error("unique key or primary key constraint failed. Sqlite error {code} : {error_msg}")]
    UniqueConstraint { code: c_int, error_msg: String },

    /// A CHECK constraint was violated during execution.
    #[error("Constraint check failed. Sqlite error {code} : {error_msg}")]
    CheckConstraint { code: c_int, error_msg: String },

    /// A general SQLite execution failure.
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },
}