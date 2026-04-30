use std::ffi::c_int;

#[derive(thiserror::Error, Debug)]
pub enum StatementStepErrors {
    #[error("SqliteBusy. Operation took more than 5 seconds")]
    SqliteBusy,

    #[error("Foreign key constraint failed. Sqlite error {code} : {error_msg}")]
    ForeignKeyConstraint { code: c_int, error_msg: String },

    #[error("unique key or primary key constraint failed. Sqlite error {code} : {error_msg}")]
    UniqueConstraint { code: c_int, error_msg: String },

    #[error("Constraint check failed. Sqlite error {code} : {error_msg}")]
    CheckConstraint { code: c_int, error_msg: String },

    // in case of any other errors
    #[error("SQLite error {code}: {error_msg}")]
    SqliteFailure { code: c_int, error_msg: String },
}
