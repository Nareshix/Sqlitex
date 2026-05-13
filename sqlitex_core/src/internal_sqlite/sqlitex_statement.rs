use libsqlite3_sys::{sqlite3_finalize, sqlite3_stmt};

#[allow(unused)]
pub struct SqlitexStmt {
    pub sql_query: &'static str,
    pub stmt: *mut sqlite3_stmt,
}

// sqlite default mode is serialized
unsafe impl Send for SqlitexStmt {}
unsafe impl Sync for SqlitexStmt {}

impl Drop for SqlitexStmt {
    fn drop(&mut self) {
        // If the statement was initialized, we must finalize it to prevent memory leaks.
        if !self.stmt.is_null() {
            unsafe {
                sqlite3_finalize(self.stmt);
            }
        }
    }
}
