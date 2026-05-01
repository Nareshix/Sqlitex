use sqlitex::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;

    conn.execute_runtime("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT UNIQUE)")?;

    // Successful Transaction
    let user_count = conn.transaction(|tx| {
        tx.execute_runtime("INSERT INTO users (name) VALUES ('Alice')")?;
        tx.execute_runtime("INSERT INTO users (name) VALUES ('Bob')")?;

        let row = tx
            .query_runtime("SELECT COUNT(*) FROM users")?
            .first()?
            .unwrap();
        Ok(row[0].as_i32()) // Return the count
    })?;

    println!("{}", user_count); // Prints 2

    // 3. Failed Transaction (Automatic Rollback)
    // We try to add Charlie, then Alice again (who already exists).
    let result = conn.transaction(|tx| {
        tx.execute_runtime("INSERT INTO users (name) VALUES ('Charlie')")?; // Succeeds
        tx.execute_runtime("INSERT INTO users (name) VALUES ('Alice')")?; // Fails (UNIQUE constraint)
        Ok(())
    });

    if let Err(e) = result {
        println!("{}", e);
    }

    // Charlie should NOT exist in the DB because the transaction reverted.
    let final_count = conn
        .query_runtime("SELECT COUNT(*) FROM users")?
        .first()?
        .unwrap()[0]
        .as_i32();

    println!("Charlie not added. Total count: {}", final_count); // prints 2 since Charlie was not added.

    Ok(())
}


