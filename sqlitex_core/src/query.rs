
use crate::{
    errors::{Error, SqlReadErrorBindings, SqlWriteBindingError, SqliteFailure},
    internal_sqlite::{
        preparred_statement::PreparredStmt,
        rows_dao::Rows,
        sqlitex_connection::Connection,
    },
    traits::row_mapper::RowMapper,
};

/// Runner for write statements (`INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE`).
/// Only exposes `.execute(&conn)`.
pub struct WriteQuery<B> {
    pub sql: &'static str,
    pub binder: B,
}

impl<B: FnOnce(&PreparredStmt) -> Result<(), SqliteFailure>> WriteQuery<B> {
    pub fn execute(self, conn: &Connection) -> Result<u64, Error> {
        let mut stmt = std::ptr::null_mut();
        unsafe {
            crate::utility::utils::prepare_stmt(conn.db, &mut stmt, self.sql)
                .map_err(|e| Error::from(SqlWriteBindingError::Prepare(e)))?;
        }
        let mut prep = PreparredStmt {
            stmt,
            conn: conn.db,
        };
        (self.binder)(&prep).map_err(|e| Error::from(SqlWriteBindingError::Bind(e)))?;
        prep.step().map_err(|e| Error::from(SqlWriteBindingError::Step(e)))?;
        let changes = unsafe { libsqlite3_sys::sqlite3_changes(conn.db) };
        Ok(changes as u64)
    }
}

/// Runner for queries that mathematically return **exactly 1 row** (e.g. `COUNT(*)` without `GROUP BY`).
/// Only exposes `.fetch_one(&conn)`.
pub struct ScalarQuery<M, B> {
    pub sql: &'static str,
    pub mapper: M,
    pub binder: B,
}

impl<M: RowMapper, B: FnOnce(&PreparredStmt) -> Result<(), SqliteFailure>> ScalarQuery<M, B> {
    pub fn fetch_one(self, conn: &Connection) -> Result<M::Output, Error> {
        let mut stmt = std::ptr::null_mut();
        unsafe {
            crate::utility::utils::prepare_stmt(conn.db, &mut stmt, self.sql)
                .map_err(|e| Error::from(SqlReadErrorBindings::Prepare(e)))?;
        }
        let prep = PreparredStmt {
            stmt,
            conn: conn.db,
        };
        (self.binder)(&prep).map_err(|e| Error::from(SqlReadErrorBindings::Bind(e)))?;
        prep.query(self.mapper)
            .first()
            .map_err(Error::from)?
            .ok_or(Error::RowNotFound)
    }
}

/// Runner for queries with unique constraints or `LIMIT 1` (`WHERE id = ?`).
/// Exposes `.fetch_optional(&conn)` and `.fetch_one(&conn)`.
pub struct OptionalQuery<M, B> {
    pub sql: &'static str,
    pub mapper: M,
    pub binder: B,
}

impl<M: RowMapper, B: FnOnce(&PreparredStmt) -> Result<(), SqliteFailure>> OptionalQuery<M, B> {
    pub fn fetch<'a>(self, conn: &'a Connection) -> Result<Rows<'a, M>, Error> {
        let mut stmt = std::ptr::null_mut();
        unsafe {
            crate::utility::utils::prepare_stmt(conn.db, &mut stmt, self.sql)
                .map_err(|e| Error::from(SqlReadErrorBindings::Prepare(e)))?;
        }
        let prep = PreparredStmt {
            stmt,
            conn: conn.db,
        };
        (self.binder)(&prep).map_err(|e| Error::from(SqlReadErrorBindings::Bind(e)))?;
        Ok(prep.query(self.mapper))
    }

    pub fn fetch_optional(self, conn: &Connection) -> Result<Option<M::Output>, Error> {
        self.fetch(conn)?.first().map_err(Error::from)
    }

    pub fn fetch_one(self, conn: &Connection) -> Result<M::Output, Error> {
        self.fetch_optional(conn)?
            .ok_or(Error::RowNotFound)
    }
}

/// Runner for queries that can return multiple rows (`SELECT * FROM ...`).
/// Exposes `.fetch_all()`, `.fetch()`, `.fetch_optional()`, and `.fetch_one()`.
pub struct ManyQuery<M, B> {
    pub sql: &'static str,
    pub mapper: M,
    pub binder: B,
}

impl<M: RowMapper, B: FnOnce(&PreparredStmt) -> Result<(), SqliteFailure>> ManyQuery<M, B> {
    pub fn fetch<'a>(self, conn: &'a Connection) -> Result<Rows<'a, M>, Error> {
        let mut stmt = std::ptr::null_mut();
        unsafe {
            crate::utility::utils::prepare_stmt(conn.db, &mut stmt, self.sql)
                .map_err(|e| Error::from(SqlReadErrorBindings::Prepare(e)))?;
        }
        let prep = PreparredStmt {
            stmt,
            conn: conn.db,
        };
        (self.binder)(&prep).map_err(|e| Error::from(SqlReadErrorBindings::Bind(e)))?;
        Ok(prep.query(self.mapper))
    }

    pub fn fetch_all(self, conn: &Connection) -> Result<Vec<M::Output>, Error> {
        self.fetch(conn)?.all().map_err(Error::from)
    }

    pub fn fetch_optional(self, conn: &Connection) -> Result<Option<M::Output>, Error> {
        self.fetch(conn)?.first().map_err(Error::from)
    }

    pub fn fetch_one(self, conn: &Connection) -> Result<M::Output, Error> {
        self.fetch_optional(conn)?
            .ok_or(Error::RowNotFound)
    }
}