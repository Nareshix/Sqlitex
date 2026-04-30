use libsqlite3_sys::{
    SQLITE_BUSY, SQLITE_DONE, SQLITE_FLOAT, SQLITE_INTEGER, SQLITE_ROW, SQLITE_TEXT, sqlite3,
    sqlite3_column_count, sqlite3_column_double, sqlite3_column_int64, sqlite3_column_text,
    sqlite3_column_type, sqlite3_finalize, sqlite3_step, sqlite3_stmt,
};
use std::ffi::CStr;

use crate::{
    errors::row::RowMapperError, traits::dynamic::Value, utility::utils::get_sqlite_failiure,
};

pub struct DynamicRows {
    stmt: *mut sqlite3_stmt,
    conn: *mut sqlite3,
    pub column_names: Vec<String>,
}

impl DynamicRows {
    pub fn new(stmt: *mut sqlite3_stmt, conn: *mut sqlite3, column_names: Vec<String>) -> Self {
        DynamicRows {
            stmt,
            conn,
            column_names,
        }
    }
}

impl Drop for DynamicRows {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.stmt);
        }
    }
}

impl Iterator for DynamicRows {
    type Item = Result<Vec<Value>, RowMapperError>;

    fn next(&mut self) -> Option<Self::Item> {
        let result_code = unsafe { sqlite3_step(self.stmt) };

        if result_code == SQLITE_ROW {
            let count = unsafe { sqlite3_column_count(self.stmt) };
            let mut row = Vec::with_capacity(count as usize);

            for i in 0..count {
                let val = unsafe {
                    match sqlite3_column_type(self.stmt, i) {
                        SQLITE_INTEGER => Value::Integer(sqlite3_column_int64(self.stmt, i)),
                        SQLITE_FLOAT => Value::Real(sqlite3_column_double(self.stmt, i)),
                        SQLITE_TEXT => {
                            let ptr = sqlite3_column_text(self.stmt, i);
                            if ptr.is_null() {
                                Value::Null
                            } else {
                                let s = CStr::from_ptr(ptr as *const i8)
                                    .to_string_lossy()
                                    .into_owned();
                                Value::Text(s)
                            }
                        }
                        // TODO BLOB
                        _ => Value::Null,
                    }
                };
                row.push(val);
            }
            Some(Ok(row))
        } else if result_code == SQLITE_BUSY {
            Some(Err(RowMapperError::SqliteBusy))
        } else if result_code == SQLITE_DONE {
            None
        } else {
            let (code, error_msg) = unsafe { get_sqlite_failiure(self.conn) };
            Some(Err(RowMapperError::SqliteFailure { code, error_msg }))
        }
    }
}

impl DynamicRows {
    /// Returns the first row if available, or `None` if the query returned no results.
    pub fn first(mut self) -> Result<Option<Vec<Value>>, RowMapperError> {
        self.next().transpose()
    }

    /// Collects the iterator into a vector of rows.
    pub fn all(self) -> Result<Vec<Vec<Value>>, RowMapperError> {
        self.collect()
    }
}
