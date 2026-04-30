use std::marker::PhantomData;

use libsqlite3_sys::{SQLITE_BUSY, SQLITE_DONE, SQLITE_ROW, sqlite3_step};

use crate::{
    errors::row::RowMapperError, internal_sqlite::preparred_statement::PreparredStmt,
    traits::row_mapper::RowMapper, utility::utils::get_sqlite_failiure,
};

#[allow(dead_code)]
pub struct Rows<'a, M: RowMapper> {
    pub stmt: PreparredStmt,
    pub mapper: M,
    pub _marker: PhantomData<&'a mut ()>,
}

impl<'a, M: RowMapper> Iterator for Rows<'a, M> {
    // The Output refers to the original struct predefined by user (TODO, better explanation)
    type Item = Result<M::Output, RowMapperError>;

    fn next(&mut self) -> Option<Self::Item> {
        let result_code = unsafe { sqlite3_step(self.stmt.stmt) };

        if result_code == SQLITE_ROW {
            // Call the map_row method on our stored mapper instance.
            let item = unsafe { self.mapper.map_row(self.stmt.stmt) };
            Some(Ok(item))
        } else if result_code == SQLITE_BUSY {
            Some(Err(RowMapperError::SqliteBusy))
        } else if result_code == SQLITE_DONE {
            None
        } else {
            let (code, error_msg) = unsafe { get_sqlite_failiure(self.stmt.conn) };
            Some(Err(RowMapperError::SqliteFailure { code, error_msg }))
        }
    }
}

impl<'a, M: RowMapper> Rows<'a, M> {
    /// Returns the first row if available, or `None` if the query returned no results.
    ///
    /// # Example
    /// ```
    /// let results = db.get_active_users(false)?;
    /// let first_result = results.first()?.unwrap(); // returns first column from the returned rows
    /// ```
    pub fn first(mut self) -> Result<Option<M::Output>, RowMapperError> {
        self.next().transpose()
    }

    /// collects the iterator into a vector. Just a lightweight wrapper around `.collect()`
    /// to prevent adding type hints (`Vec<_>`) in code
    ///
    /// # Example
    /// ```
    /// let results = db.get_active_users(false)?;
    /// let collected_results =results.all()?; // returns a vec of owned results from the returned rows
    /// ```
    pub fn all(self) -> Result<Vec<M::Output>, RowMapperError> {
        self.collect()
    }
}
