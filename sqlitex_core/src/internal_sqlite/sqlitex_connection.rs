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
    /// Opens a SQLite database from a file path.
    ///
    /// If the database file does not exist, it will be created automatically.
    ///
    /// This opens the database with read-write access enabled.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteOpenErrors`] if the database cannot be opened.
    ///
    /// # Example
    ///
    /// ```rust
    /// let db = Connection::open("app.db")?;
    /// ```
    pub fn open(filename: &str) -> Result<Arc<Self>, SqliteOpenErrors> {
        let flag = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
        Connection::open_with_flags(filename, flag)
    }

    /// Opens an in-memory SQLite database.
    ///
    /// The database exists only in memory and is destroyed when the connection
    /// is dropped.
    ///
    /// This is useful for testing, temporary computations, or ephemeral data.
    ///
    /// # Returns
    ///
    /// Returns an [`Arc<Self>`] wrapped in-memory connection.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteOpenErrors`] if the database cannot be created.
    ///
    /// # Example
    ///
    /// ```rust
    /// let db = Connection::open_memory()?;
    /// ```
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

    /// Alias to [`execute_batch`]. avoid using this as it will be deprecated soon.
    /// Executes multiple SQL statements in a single string.
    /// ---
    /// This is useful for running batches of statements
    ///
    /// Unlike [`execute`] or [`query`], this method allows running more than one
    /// SQL statement at once.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteFailure`] if any statement fails during execution.
    ///
    /// # Example
    ///
    /// ```rust
    /// db.execute_batch("
    ///     CREATE TABLE users(id INTEGER);
    ///     INSERT INTO users VALUES (1);
    ///     SELECT * FROM users;
    /// ")?;
    /// ```
    // TODO Do not delete this function for now. many macros depend on this function. eventually delte it as it is the same as as executre_batch_runtime
    #[deprecated(since = "0.2.3", note = "Use `execute_script` instead")]
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

    /// Executes multiple SQL statements in a single string.
    ///
    /// This is useful for running batches of statements
    ///
    /// Unlike [`execute`] or [`query`], this method allows running more than one
    /// SQL statement at once.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteFailure`] if any statement fails during execution.
    ///
    /// # Example
    ///
    /// ```rust
    /// db.execute_batch("
    ///     CREATE TABLE users(id INTEGER);
    ///     INSERT INTO users VALUES (1);
    ///     SELECT * FROM users;
    /// ")?;
    /// ```
    pub fn execute_batch(&self, sql: &str) -> Result<(), SqliteFailure> {
        self.exec(sql)?;
        Ok(())
    }

    /// Executes a runtime `SELECT` query and returns the resulting rows.
    ///
    /// This method is intended for read operations and returns a [`DynamicRows`]
    /// iterator over the result set.
    ///
    /// Only a single SQL statement is allowed. Chaining multiple statements
    /// using `;` is not supported. For executing multiple statements, see
    /// [`execute_batch`].
    ///
    /// # Returns
    ///
    /// Returns a [`DynamicRows`] handle that can be used to iterate over
    /// the query results.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteFailure`] if:
    /// - the SQL statement fails to prepare
    /// - the query is invalid or violates SQLite constraints
    ///
    /// # Example
    ///
    /// ```rust
    /// let rows = db.query("SELECT id, name FROM users")?;
    /// ```
    ///
    /// # See also
    ///
    /// - [`execute`] for write operations (`INSERT`, `UPDATE`, `DELETE`)
    /// - [`execute_batch`] for executing multiple SQL statements
    pub fn query(&self, sql: &str) -> Result<DynamicRows, SqliteFailure> {
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

    /// Executes a runtime SQL statement that modifies the database.
    ///
    /// This method is intended for write operations such as
    /// `CREATE`, `INSERT`, `UPDATE`, and `DELETE`.
    ///
    ///
    /// # Returns
    ///
    /// Returns the **number** of rows modified by the statement if successful.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteFailure`] if:
    /// - the SQL statement fails to prepare
    /// - execution fails (e.g. constraint violation, syntax error)
    ///
    /// # Example
    ///
    /// ```rust
    /// let rows = db.execute("UPDATE users SET active = 1")?;
    /// println!("{} rows updated", rows);
    /// ```
    ///
    /// # Notes
    ///
    /// - This method executes exactly one statement.
    /// - To run multiple SQL statements at once, see [`execute_batch`].
    pub fn execute(&self, sql: &str) -> Result<u64, SqliteFailure> {
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

    /// Executes multiple database operations inside a single transaction.
    ///
    /// If the closure returns `Ok`, the transaction is committed.
    ///
    /// If the closure returns `Err`, the transaction is rolled back.
    ///
    /// # Example
    ///
    /// db.transaction(|tx| {
    ///     tx.insert_user("Alice")?;
    ///     tx.insert_post("Hello")?;
    ///     Ok(())
    /// })?;
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
