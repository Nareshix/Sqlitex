use libsqlite3_sys::{
    self as ffi, SQLITE_OK, sqlite3,
    sqlite3_errcode, sqlite3_stmt,
};
use std::{
    ffi::{CStr, CString, c_char},
    ptr,
};

use crate::errors::connection::SqlitePrepareErrors;

pub enum RustTypes {
    Integer,
    String,
    Float,
    Null,
}

// pub fn sqlite_to_rust_type_mapping(sqlite_type: i32) -> Result<RustTypes,SqliteToRustConversionFailiure> {
//     match sqlite_type {
//         SQLITE_INTEGER => Ok(RustTypes::Integer),
//         SQLITE_FLOAT => Ok(RustTypes::Float),
//         SQLITE_TEXT => Ok(RustTypes::String),
//         SQLITE_NULL => Ok(RustTypes::Null),
//         _ => Err(SqliteToRustConversionFailiure),
//     }
// }

/// Internally calls sqlite3_errcode and sqlite3_errmsg to return
/// specifcally **Error::SqliteFailure** with the necessary code and error_msg
///
///  # Safety
///
/// - db must be a valid sqlite3 connection which is not NULL
pub unsafe fn get_sqlite_failiure(db: *mut sqlite3) -> (i32, String) {
    let safe_error_msg = unsafe {
        // sqlite internally handles dropping c_error_msg
        let c_error_msg = ffi::sqlite3_errmsg(db);
        CStr::from_ptr(c_error_msg as *const c_char)
    };

    let error_msg = safe_error_msg.to_string_lossy().into_owned();
    let code = unsafe { sqlite3_errcode(db) };

    (code, error_msg)
}

///
/// # Safety
///
/// - db must be a valid sqlite3 connection which is not NULL
pub unsafe fn close_db(db: *mut sqlite3) {
    // also sqlite3_close_v2 is only for gc languages hence sqlite3_close is preferred
    unsafe { ffi::sqlite3_close(db) };
}

///# Safety
/// db must be a valid pointer
pub unsafe fn prepare_stmt(
    db: *mut sqlite3,
    stmt: &mut *mut sqlite3_stmt,
    sql: &str,
) -> Result<(), SqlitePrepareErrors> {
    let c_sql_query =
        CString::new(sql).expect("SQL statement or filename should not contain null bytes"); // will never fail, but still good to add the expect
    let code =
        unsafe { ffi::sqlite3_prepare_v2(db, c_sql_query.as_ptr(), -1, stmt, ptr::null_mut()) };

    if code != SQLITE_OK {
        let (code, error_msg) = unsafe { get_sqlite_failiure(db) };
        return Err(SqlitePrepareErrors::SqliteFailure { code, error_msg });
    }
    Ok(())
}