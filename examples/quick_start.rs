use sqlitex::{Connection, sqlitex};


// The examples and docs would uses inline schema (#[sqlitex] with no args) to keep things self-contained.
// For real projects, prefer pointing to an external file instead as it is more flexible:
//   #[sqlitex("schema.sql")]      — point to a .sql file with CREATE TABLE statements
//   #[sqlitex("existing.db")]     — point to an existing database file
// See: https://docs.rs/sqlitex/latest/sqlitex/#connection-methods
#[sqlitex]
struct AppDatabase {
    // all create tables must be at the top before read/write logic in order to get compile time checks
    // or else you will get compile-time errors.
    // Alternatively, You could point to a .sql file or an existing db and skip the create table statements in the struct


    // It is not recommended to use STRICT table. Read up more here: https://docs.rs/sqlitex/latest/sqlitex/#important-note-on-strict-tables
    // you don't have to import sql! macro. #[sqlitex] brings with it
    init: sql!("
    -- Note the `NOT NULL` constraints which allows us to use concrete types instead of Option<T>, (e.g., `i32` instead of `Option<i32>`)
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            -- note that SQLite doesn't have a native boolean type.
            -- Creating table with `BOOLEAN` and `BOOL` data type aliases to `INTEGER CHECK (col IN (0, 1))`
            -- which maps to bool datatype in rust
            is_active BOOL NOT NULL
        )
    "),

    // postgres `::` type casting is supported. Alternatively u can use `CAST AS` syntax
    add_user: sql!("INSERT INTO users (id, username, is_active) VALUES (?::REAL, ?, ?);"),

    // or `id::REAL` instead of `CAST (id AS REAL)`
    get_active_users: sql!("SELECT CAST (id AS REAL), username, is_active as active FROM users WHERE is_active = ?"),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `sqlitex` automatically uses the best sqlite setting for max performance and reliability (e.g. wal mode, synchronous=normal and more)
    // when opening a connection

    // or Connection::open("path/to/sql.db")  note that it lazily creates one if doesnt exist
    let conn = Connection::open_memory()?;

    // The 'new' constructor is generated automatically
    let mut db = AppDatabase::new(conn);

    // You can now call the methods and it will run the sql commands
    db.init()?;

    // Types are enforced by Rust
    // Respects type inference. i64 -> f64 for id (first argument)
    db.add_user(0.0, "Alice", true)?;
    db.add_user(1.0, "Bob", false)?;

    // active_users is an iterator.
    // first() and all() methods are additionally provided.
    let active_users = db.get_active_users(true)?;

    for user in active_users {
        // u can access the fields specifically if you want
        // Respects aliases (is_active -> active)
        let user = user?;
        println!("{}, {}, {}", user.id, user.username, user.active); // note user.id is float as we type casted it in the sql stmt
    }

    Ok(())
    // prints out "0, Alice, true"
}
