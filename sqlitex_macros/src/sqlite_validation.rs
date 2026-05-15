use sqlitex_core::libsqlite3_sys::{
    self as ffi, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_MEMORY, SQLITE_OPEN_READONLY,
    SQLITE_OPEN_READWRITE, SQLITE_ROW, sqlite3, sqlite3_close, sqlite3_column_text,
    sqlite3_exec, sqlite3_finalize, sqlite3_free, sqlite3_open_v2,
    sqlite3_prepare_v2, sqlite3_step, sqlite3_stmt,
};
use sqlitex_core::utility::utils::get_sqlite_failiure;
use sqlitex_type_inference::{expr::BaseType, table::ColumnInfo};
use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    fs,
    path::Path,
    ptr,
};


struct SqliteHandle {
    db: *mut sqlite3,
}

impl Drop for SqliteHandle {
    fn drop(&mut self) {
        unsafe {
            if !self.db.is_null() {
                sqlite3_close(self.db);
            }
        }
    }
}

impl SqliteHandle {
    fn open(db_path: &str) -> Result<Self, String> {
        let path = Path::new(db_path);
        if !path.exists() {
            return Err(format!("File not found at path: '{}'", db_path));
        }

        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let is_sql_script = extension.eq_ignore_ascii_case("sql");
        let mut db = ptr::null_mut();

        unsafe {
            let rc = if is_sql_script {
                let memory_path = CString::new(":memory:").unwrap();
                let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MEMORY;
                sqlite3_open_v2(memory_path.as_ptr(), &mut db, flags, ptr::null())
            } else {
                let c_path =
                    CString::new(db_path).map_err(|_| "Path contains nulls".to_string())?;
                let flags = SQLITE_OPEN_READONLY;
                sqlite3_open_v2(c_path.as_ptr(), &mut db, flags, ptr::null())
            };

            if rc != SQLITE_OK {
                let (_, msg) = get_sqlite_failiure(db);

                // raii to work. so it auto closes
                let _ = SqliteHandle { db };
                return Err(format!("Failed to open DB: {}", msg));
            }

            // Wrap in RAII struct immediately so 'Drop' handles closing automatically
            let handle = SqliteHandle { db };

            if is_sql_script {
                let file_content = fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read .sql file: {}", e))?;

                let c_sql = CString::new(file_content)
                    .map_err(|_| "SQL file contains illegal null bytes".to_string())?;

                let mut err_msg: *mut c_char = ptr::null_mut();
                let exec_rc = sqlite3_exec(db, c_sql.as_ptr(), None, ptr::null_mut(), &mut err_msg);

                if exec_rc != SQLITE_OK {
                    let msg = if !err_msg.is_null() {
                        let m = CStr::from_ptr(err_msg).to_string_lossy().into_owned();
                        sqlite3_free(err_msg as *mut c_void);
                        m
                    } else {
                        "Unknown error".to_string()
                    };
                    return Err(format!("Error in .sql script: {}", msg));
                }
            }

            Ok(handle)
        }
    }
    fn open_memory() -> Result<Self, String> {
        unsafe {
            let mut db = ptr::null_mut();
            let memory_path = CString::new(":memory:").unwrap();
            let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MEMORY;

            let rc = sqlite3_open_v2(memory_path.as_ptr(), &mut db, flags, ptr::null());

            if rc != SQLITE_OK {
                let (_, msg) = get_sqlite_failiure(db); // using your existing helper
                return Err(format!("Failed to open memory DB: {}", msg));
            }

            Ok(SqliteHandle { db })
        }
    }
}


pub fn get_db_schema_from_statements(scripts: &[(String, String)]) -> Result<Vec<String>, String> {
    let handle = SqliteHandle::open_memory()?;

    struct StmtGuard(*mut ffi::sqlite3_stmt);
    impl Drop for StmtGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    ffi::sqlite3_finalize(self.0);
                }
            }
        }
    }

    for (filename, sql) in scripts {
        let c_sql = CString::new(sql.as_str())
            .map_err(|_| format!("File '{}' contains illegal null bytes", filename))?;

        // start our pointer at the beginning of the SQL string
        let mut pz_tail = c_sql.as_ptr();

        // Iterate through the file, statement by statement
        while unsafe { *pz_tail } != 0 {
            let z_sql = pz_tail;
            let mut raw_stmt: *mut ffi::sqlite3_stmt = ptr::null_mut();

            // Parse the next statement. pz_tail is advanced to the start of the NEXT statement.
            let prepare_rc = unsafe {
                ffi::sqlite3_prepare_v2(handle.db, z_sql, -1, &mut raw_stmt, &mut pz_tail)
            };

            // Wrap immediately in RAII guard so it ALWAYS gets finalized, even on panic/early return
            let stmt = StmtGuard(raw_stmt);

            // Calculate Line Number
            // Find how many bytes into the string we are
            let byte_offset = (z_sql as usize).saturating_sub(c_sql.as_ptr() as usize);

            // Ensure we don't slice inside a multi-byte UTF-8 character
            let mut safe_offset = byte_offset.min(sql.len());
            while safe_offset > 0 && !sql.is_char_boundary(safe_offset) {
                safe_offset -= 1;
            }
            // Count the newlines up to this point
            let line_number = sql[..safe_offset].matches('\n').count() + 1;

            if prepare_rc != ffi::SQLITE_OK {
                let (_, msg) = unsafe { get_sqlite_failiure(handle.db) };
                return Err(format!(
                    "In file '{}' at line {}: {}",
                    filename, line_number, msg
                ));
            }

            // If the statement is empty (e.g. trailing whitespace or comments)
            if stmt.0.is_null() {
                continue;
            }

            loop {
                let step_rc = unsafe { ffi::sqlite3_step(stmt.0) };

                if step_rc == ffi::SQLITE_DONE {
                    break;
                } else if step_rc != ffi::SQLITE_ROW {
                    let (_, msg) = unsafe { get_sqlite_failiure(handle.db) };
                    return Err(format!(
                        "In file '{}' at line {}: {}",
                        filename, line_number, msg
                    ));
                }
            }
        }
    }

    let query = b"SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name\0";
    let mut raw_stmt = ptr::null_mut();

    unsafe {
        if ffi::sqlite3_prepare_v2(
            handle.db,
            query.as_ptr() as *const c_char,
            -1,
            &mut raw_stmt,
            ptr::null_mut(),
        ) != ffi::SQLITE_OK
        {
            let (_, msg) = get_sqlite_failiure(handle.db);
            return Err(msg);
        }
    }

    let stmt = StmtGuard(raw_stmt);
    let mut results = Vec::new();

    unsafe {
        while ffi::sqlite3_step(stmt.0) == ffi::SQLITE_ROW {
            let sql_ptr = ffi::sqlite3_column_text(stmt.0, 1);
            if !sql_ptr.is_null() {
                results.push(
                    CStr::from_ptr(sql_ptr as *const c_char)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }

    Ok(results)
}

pub fn get_db_schema(db_path: &str) -> Result<Vec<String>, String> {
    let handle = SqliteHandle::open(db_path)?;

    unsafe {
        let sql = b"SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name\0";
        let mut stmt: *mut sqlite3_stmt = ptr::null_mut();

        let prepare_rc = sqlite3_prepare_v2(
            handle.db,
            sql.as_ptr() as *const c_char,
            -1,
            &mut stmt,
            ptr::null_mut(),
        );

        if prepare_rc != SQLITE_OK {
            let (_, msg) = get_sqlite_failiure(handle.db);
            return Err(msg);
        }

        struct StmtGuard(*mut sqlite3_stmt);
        impl Drop for StmtGuard {
            fn drop(&mut self) {
                unsafe {
                    sqlite3_finalize(self.0);
                }
            }
        }
        let _guard = StmtGuard(stmt);

        let mut results = Vec::new();
        while sqlite3_step(stmt) == SQLITE_ROW {
            let sql_ptr = sqlite3_column_text(stmt, 1);
            if !sql_ptr.is_null() {
                results.push(
                    CStr::from_ptr(sql_ptr as *const c_char)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }

        Ok(results)
    }
}

pub fn validate_sql_syntax_with_sqlite(
    tables: &HashMap<String, Vec<ColumnInfo>>,
    sql: &str,
) -> Result<(), String> {
    let handle = SqliteHandle::open_memory()?;

    unsafe {
        for (table_name, columns) in tables {
            let col_defs: Vec<String> = columns
                .iter()
                .map(|col| {
                    let sql_type = match col.data_type.base_type {
                        BaseType::Integer => "INTEGER",
                        BaseType::Real => "REAL",
                        BaseType::Bool => "BOOLEAN",
                        BaseType::Text => "TEXT",
                        BaseType::Blob => "BLOB",
                        BaseType::Null | BaseType::Unknowns | BaseType::PlaceHolder => "TEXT",
                    };

                    let constraint = if !col.data_type.nullable {
                        " NOT NULL"
                    } else {
                        ""
                    };

                    format!("{} {}{}", col.name, sql_type, constraint)
                })
                .collect();

            let create_stmt = format!("CREATE TABLE {} ({});", table_name, col_defs.join(", "));

            let c_create_sql = CString::new(create_stmt).unwrap();
            let mut err_msg: *mut c_char = ptr::null_mut();
            let rc = sqlite3_exec(
                handle.db,
                c_create_sql.as_ptr(),
                None,
                ptr::null_mut(),
                &mut err_msg,
            );

            if rc != SQLITE_OK {
                if !err_msg.is_null() {
                    sqlite3_free(err_msg as *mut c_void);
                }
                return Err(format!("Failed to recreate table schema: {}", table_name));
            }
        }
        let c_sql = CString::new(sql).map_err(|_| "Invalid SQL string".to_string())?;
        let mut stmt = ptr::null_mut();

        let prepare_rc =
            sqlite3_prepare_v2(handle.db, c_sql.as_ptr(), -1, &mut stmt, ptr::null_mut());

        if !stmt.is_null() {
            sqlite3_finalize(stmt);
        }

        if prepare_rc == SQLITE_OK {
            Ok(())
        } else {
            let (_, msg) = get_sqlite_failiure(handle.db);
            Err(msg.to_string())
        }
    }
}

