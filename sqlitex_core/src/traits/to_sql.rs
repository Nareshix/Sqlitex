use libsqlite3_sys::{self as ffi, SQLITE_TRANSIENT, sqlite3_stmt};

// BLOB not implemented yet. TODO
pub trait ToSql {
    /// - it is ok for it to be self consuming (tho it only applies to String)
    ///   because we are not gonna be using this rust type anymore
    /// - Indexes start at 1
    ///# Safety
    ///
    /// Pass in the stmt pointer (not the address). it uses sqlite3_bind_* to bind it to the correct type
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32;
}

impl ToSql for String {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        unsafe { self.as_str().bind_to(stmt, index) }
    }
}

impl ToSql for &str {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        let bytes = self.as_bytes();
        let len = bytes.len() as i32;

        unsafe {
            ffi::sqlite3_bind_text(
                stmt,
                index,
                bytes.as_ptr() as *const _,
                len,
                SQLITE_TRANSIENT(),
            )
        }
    }
}

impl ToSql for i32 {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        unsafe { ffi::sqlite3_bind_int(stmt, index, self) }
    }
}

impl ToSql for i64 {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        unsafe { ffi::sqlite3_bind_int64(stmt, index, self) }
    }
}

impl ToSql for f64 {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        unsafe { ffi::sqlite3_bind_double(stmt, index, self) }
    }
}

impl ToSql for bool {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        // true as i32 == 1
        // false as i32 == 0
        let value = self as i32;

        unsafe { ffi::sqlite3_bind_int(stmt, index, value) }
    }
}

impl<T: ToSql> ToSql for Option<T> {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        match self {
            Some(rust_value) => unsafe { rust_value.bind_to(stmt, index) },
            None => unsafe { ffi::sqlite3_bind_null(stmt, index) },
        }
    }
}

impl ToSql for &[u8] {
    unsafe fn bind_to(self, stmt: *mut sqlite3_stmt, index: i32) -> i32 {
        unsafe {
            ffi::sqlite3_bind_blob(
                stmt,
                index,
                self.as_ptr() as *const _,
                self.len() as i32,
                ffi::SQLITE_TRANSIENT(),
            )
        }
    }
}