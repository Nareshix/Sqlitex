use libsqlite3_sys::sqlite3_stmt;

pub trait RowMapper {
    type Output;

    /// # Safety
    /// The caller must ensure the statement has a row ready to be read.
    unsafe fn map_row(&self, stmt: *mut sqlite3_stmt) -> Self::Output;
}
