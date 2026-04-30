use std::ffi::CStr;

use libsqlite3_sys::{SQLITE_NULL, sqlite3_column_type, sqlite3_stmt};

// no errors cuz sqlite does implicit conversion
pub trait FromSql {
    /// # Safety
    /// The caller must ensure that `stmt` points to a valid, stepped statement
    /// that is currently on a row (i.e., sqlite3_step has returned SQLITE_ROW).
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self;
}

impl FromSql for String {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        let c_string = unsafe { libsqlite3_sys::sqlite3_column_text(stmt, index) } as *const i8;
        unsafe { CStr::from_ptr(c_string).to_string_lossy().into_owned() }
    }
}

// impl FromSql for &str {
//     unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
//         let c_string = unsafe { libsqlite3_sys::sqlite3_column_text(stmt, index) } as *const i8;
//         unsafe { CStr::from_ptr(c_string).to_string_lossy().into_owned() }
//     }
// }

impl FromSql for f64 {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        unsafe { libsqlite3_sys::sqlite3_column_double(stmt, index) }
    }
}

impl FromSql for i32 {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        unsafe { libsqlite3_sys::sqlite3_column_int(stmt, index) }
    }
}
impl FromSql for i64 {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        unsafe { libsqlite3_sys::sqlite3_column_int64(stmt, index) }
    }
}

impl FromSql for bool {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        let val = unsafe { libsqlite3_sys::sqlite3_column_int(stmt, index) };
        val != 0
    }
}

impl<T: FromSql> FromSql for Option<T> {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        let column_type = unsafe { sqlite3_column_type(stmt, index) };

        if column_type == SQLITE_NULL {
            None
        } else {
            Some(unsafe { T::from_sql(stmt, index) })
        }
    }
}

impl FromSql for Vec<u8> {
    unsafe fn from_sql(stmt: *mut sqlite3_stmt, index: i32) -> Self {
        let ptr = unsafe { libsqlite3_sys::sqlite3_column_blob(stmt, index) };
        let bytes = unsafe { libsqlite3_sys::sqlite3_column_bytes(stmt, index) };

        // The return value from sqlite3_column_blob() for a zero-length BLOB is a NULL pointer.
        // (https://sqlite.org/c3ref/column_blob.html)
        if ptr.is_null() {
            return Vec::new();
        }

        unsafe { std::slice::from_raw_parts(ptr as *const u8, bytes as usize).to_vec() }
    }
}
