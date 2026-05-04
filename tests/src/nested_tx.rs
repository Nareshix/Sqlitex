#[warn(unused)]
use sqlitex::sqlitex;

#[sqlitex]
pub struct NestedTxDao {
    create_table: sql!("CREATE TABLE IF NOT EXISTS test (id INTEGER PRIMARY KEY NOT NULL, val TEXT NOT NULL)"),
    insert: sql!("INSERT INTO test (id, val) VALUES (?, ?)"),
    count: sql!("SELECT COUNT(*) as count FROM test"),
    clear: sql!("DELETE FROM test"),
}

#[cfg(test)]
mod nested_tests {
    use std::sync::Arc;

    use super::*;

    fn setup() -> (NestedTxDao, Arc<sqlitex::Connection>) {
        let conn = sqlitex::Connection::open_memory().unwrap();
        let runtime_conn = conn.clone();
        let mut db = NestedTxDao::new(conn);
        db.create_table().unwrap();
        (db, runtime_conn)
    }

    #[test]
    fn test_macro_tx_inner_fails_outer_succeeds() -> Result<(), Box<dyn std::error::Error>> {
        let (mut db, _) = setup();

        db.transaction(|tx1| {
            tx1.insert(1, "Outer layer")?;

            let inner_result = tx1.transaction(|tx2| {
                tx2.insert(2, "Inner layer")?;
                tx2.insert(2, "Duplicate - will crash!")?;
                Ok(())
            });

            assert!(inner_result.is_err());
            Ok(())
        })?;

        let count = db.count()?.first()?.unwrap().count;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_insert_many_inside_macro_tx() -> Result<(), Box<dyn std::error::Error>> {
        let (mut db, _) = setup();

        db.transaction(|tx| {
            tx.insert(1, "Singular insert")?;
            tx.insert_many(&[
                (2, "Batch item 1".to_string()),
                (3, "Batch item 2".to_string()),
            ])?;
            Ok(())
        })?;

        let count = db.count()?.first()?.unwrap().count;
        assert_eq!(count, 3);
        Ok(())
    }

    #[test]
    fn test_runtime_inner_rollback_outer_commit() -> Result<(), Box<dyn std::error::Error>> {
        let (mut db, runtime_conn) = setup();

        runtime_conn.transaction(|conn1| {
            conn1.execute("INSERT INTO test (id, val) VALUES (1, 'Runtime Outer')")?;

            let inner_result = conn1.transaction(|conn2| {
                conn2.execute("INSERT INTO test (id, val) VALUES (2, 'Runtime Inner')")?;
                conn2.execute("INSERT INTO test (id, val) VALUES (2, 'Duplicate')")?;
                Ok(())
            });

            assert!(inner_result.is_err());
            Ok(())
        })?;

        let count = db.count()?.first()?.unwrap().count;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_mixed_macro_and_runtime_tx() -> Result<(), Box<dyn std::error::Error>> {
        let (mut db, runtime_conn) = setup();

        db.transaction(|tx| {
            tx.insert(1, "Macro Outer")?;
            runtime_conn.transaction(|conn| {
                conn.execute("INSERT INTO test (id, val) VALUES (2, 'Runtime Inner')")?;
                Ok(())
            })?;
            Ok(())
        })?;

        let count = db.count()?.first()?.unwrap().count;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn test_four_layer_deep_nesting() -> Result<(), Box<dyn std::error::Error>> {
        let (mut db, runtime_conn) = setup();

        db.transaction(|t1| {
            t1.insert(1, "L1")?;

            t1.transaction(|t2| {
                t2.insert(2, "L2")?;

                runtime_conn.transaction(|conn| {
                    conn.execute("INSERT INTO test (id, val) VALUES (3, 'L3')")?;
                    t2.insert_many(&[
                        (4, "L4A".to_string()),
                        (5, "L4B".to_string()),
                    ])?;
                    Ok(())
                })?;
                Ok(())
            })?;
            Ok(())
        })?;

        let count = db.count()?.first()?.unwrap().count;
        assert_eq!(count, 5);
        Ok(())
    }

}