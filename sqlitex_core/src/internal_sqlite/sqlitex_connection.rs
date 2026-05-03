use libsqlite3_sys::{
    self as ffi, SQLITE_DONE, SQLITE_ERROR, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_MEMORY,
    SQLITE_OPEN_READWRITE, sqlite3, sqlite3_busy_timeout, sqlite3_changes, sqlite3_column_count,
    sqlite3_column_name, sqlite3_exec, sqlite3_finalize, sqlite3_step,
};
use std::{
    ffi::{CStr, CString, c_int},
    ptr,
    sync::Arc,
};

use crate::{
    errors::{Error, connection::SqlitePrepareErrors},
    utility::utils::{close_db, get_sqlite_failiure},
};
use crate::{
    errors::{SqliteFailure, connection::SqliteOpenErrors},
    internal_sqlite::dynamic_rows::DynamicRows,
    utility::utils::prepare_stmt,
};

unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

pub struct Connection {
    pub db: *mut sqlite3,
}

impl Drop for Connection {
    fn drop(&mut self) {
        unsafe {
            close_db(self.db);
        };
    }
}

impl Connection {
    pub fn open(filename: &str) -> Result<Arc<Self>, SqliteOpenErrors> {
        let flag = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
        Connection::open_with_flags(filename, flag)
    }

    pub fn open_memory() -> Result<Arc<Self>, SqliteOpenErrors> {
        let flag = SQLITE_OPEN_MEMORY | SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
        Connection::open_with_flags(":memory:", flag)
    }

    fn open_with_flags(filename: &str, flag: c_int) -> Result<Arc<Self>, SqliteOpenErrors> {
        let mut db = ptr::null_mut();
        let c_filename = CString::new(filename).unwrap(); //TODO

        let code = unsafe { ffi::sqlite3_open_v2(c_filename.as_ptr(), &mut db, flag, ptr::null()) };

        if code == SQLITE_OK && db.is_null() {
            unsafe { close_db(db) };
            Err(SqliteOpenErrors::ConnectionAllocationFailed)
        } else if code == SQLITE_OK {
            unsafe {
                // PRAGMA busy_timeout = 5000;
                sqlite3_busy_timeout(db, 5000);

                let fk = CString::new("PRAGMA foreign_keys = ON;").unwrap();
                sqlite3_exec(db, fk.as_ptr(), None, ptr::null_mut(), ptr::null_mut());

                let wal = CString::new("PRAGMA journal_mode = WAL;").unwrap();
                sqlite3_exec(db, wal.as_ptr(), None, ptr::null_mut(), ptr::null_mut());

                let sync = CString::new("PRAGMA synchronous = NORMAL;").unwrap();
                sqlite3_exec(db, sync.as_ptr(), None, ptr::null_mut(), ptr::null_mut());
            };
            Ok(Arc::new(Self { db }))
        } else {
            let (code, error_msg) = unsafe { get_sqlite_failiure(db) };
            unsafe { close_db(db) };
            Err(SqliteOpenErrors::SqliteFailure { code, error_msg })
        }
    }

    /// Executes all SQL statements in a string. Alias to `execute_many_runtime`. avoid using this as it will be deprecated soon. TODO
    // Do not delete this function for now. many macros depend on this function
    pub fn exec(&self, sql: &str) -> Result<(), SqliteFailure> {
        let c_sql = CString::new(sql).map_err(|_| SqliteFailure {
            code: SQLITE_ERROR,
            error_msg: "SQL script contains null bytes".into(),
        })?;

        let code = unsafe {
            sqlite3_exec(
                self.db,
                c_sql.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if code != SQLITE_OK {
            let (code, error_msg) = unsafe { get_sqlite_failiure(self.db) };
            return Err(SqliteFailure { code, error_msg });
        }
        Ok(())
    }

    /// Executes all SQL statements in a string
    pub fn execute_many_runtime(&self, sql: &str) -> Result<(), SqliteFailure> {
        self.exec(sql)?;
        Ok(())
    }

    pub fn query_runtime(&self, sql: &str) -> Result<DynamicRows, SqliteFailure> {
        let mut stmt = std::ptr::null_mut();
        unsafe {
            prepare_stmt(self.db, &mut stmt, sql).map_err(|e| match e {
                SqlitePrepareErrors::SqliteFailure { code, error_msg } => {
                    SqliteFailure { code, error_msg }
                }
            })?;

            let count = sqlite3_column_count(stmt);
            let mut column_names = Vec::with_capacity(count as usize);
            for i in 0..count {
                let ptr = sqlite3_column_name(stmt, i);
                let name = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                column_names.push(name);
            }

            Ok(DynamicRows::new(stmt, self.db, column_names))
        }
    }

    pub fn execute_runtime(&self, sql: &str) -> Result<u64, SqliteFailure> {
        let mut stmt = std::ptr::null_mut();

        unsafe {
            prepare_stmt(self.db, &mut stmt, sql).map_err(|e| match e {
                SqlitePrepareErrors::SqliteFailure { code, error_msg } => {
                    SqliteFailure { code, error_msg }
                }
            })?;

            let result = sqlite3_step(stmt);
            sqlite3_finalize(stmt);

            if result == SQLITE_DONE {
                // Return how many rows were modified (e.g., "3 rows updated")
                let changes = sqlite3_changes(self.db);
                Ok(changes as u64)
            } else {
                let (code, error_msg) = get_sqlite_failiure(self.db);
                Err(SqliteFailure { code, error_msg })
            }
        }
    }

    pub fn transaction<T, F>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Self) -> Result<T, Error>,
    {
        self.exec("BEGIN IMMEDIATE").map_err(Error::from)?;

        let result = f(self);

        match result {
            Ok(val) => {
                if let Err(e) = self.exec("COMMIT") {
                    return Err(Error::from(e));
                }
                Ok(val)
            }
            Err(e) => {
                let _ = self.exec("ROLLBACK");
                Err(e)
            }
        }
    }
    // pub fn transaction_immediate<T, F>(&self, f: F) -> Result<T, Error>
    // where
    //     F: FnOnce(&Self) -> Result<T, Error>,
    // {
    //     self.exec("BEGIN IMMEDIATE").map_err(Error::from)?;

    //     let result = f(self);
    //     match result {
    //         Ok(val) => {
    //             if let Err(e) = self.exec("COMMIT") {
    //                 return Err(Error::from(e));
    //             }
    //             Ok(val)
    //         }
    //         Err(e) => {
    //             let _ = self.exec("ROLLBACK");
    //             Err(e)
    //         }
    //     }
    // }
}
