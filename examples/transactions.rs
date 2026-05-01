// Note: you cannot name a field called `transaction` in the struct since its a reserved method name.
// Failiure to do so will result in a compile time error

use sqlitex::{Connection, sqlitex};

#[sqlitex]
struct DB {
    // We add UNIQUE to trigger a real database error later
    init: sql!(
        "CREATE TABLE IF NOT EXISTS users
                (id INTEGER PRIMARY KEY NOT NULL,
                name TEXT UNIQUE NOT NULL)"
    ),

    add: sql!("INSERT INTO users (name) VALUES (?)"),

    count: sql!("SELECT count(*) as count FROM users"),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;
    let mut db = DB::new(conn);
    db.init()?;

    // Successful Transaction (Batch Commit)
    let results = db.transaction(|tx| {
        tx.add("Alice")?;
        tx.add("Bob")?;

        let count = tx.count()?.all()?;

        Ok(count) // if you are not returning anything, u should return it as `Ok(())`
    })?;

    println!("{:?}", results[0].count); // prints out '2'

    // Failed Transaction (Automatic Rollback)
    // We try to add Charlie, then add Alice again.
    // Since 'Alice' exists, the second command fails, causing the WHOLE block to revert.
    // If you are running this on ur computer, it is expected to see this in the terminal:
    // "Error: WriteBinding(Step(SqliteFailure { code: 19, error_msg: "UNIQUE constraint failed: users.name" }))"
    db.transaction(|tx| {
        tx.add("Charlie")?; // 1. Writes successfully (pending)
        tx.add("Alice")?; // 2. Fails (Duplicate) -> Triggers Rollback
        Ok(())
    })?;



    Ok(())
}

