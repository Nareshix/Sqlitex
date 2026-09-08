#[allow(unused)]
pub struct SqlitexStmt {
    pub sql_query: &'static str,
}

// sqlite default mode is serialized
unsafe impl Send for SqlitexStmt {}
unsafe impl Sync for SqlitexStmt {}