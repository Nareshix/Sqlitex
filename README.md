# Sqlitex
Sqlitex is a sqlite library for rust with compile time guarantees. It also has additional features:

- Ergonomic with excellent IDE support
- Fast. Automatically caches and reuses prepared statements for you
- Supports [BLOB](./examples/blob/) and [Transactions](./examples/transactions.rs)
- compile time guarantees for complex sql queries such as, CTEs, Window functions, Datetime functions and more
- allows fallback of [runtime features](./examples/runtime.rs) when needed


# Overview

- [Installation](#installation)
- [Feature showcase](#feature-showcase)
- [Quick Start](#quick-start)
- [Important note on STRICT tables](#important-note-on-strict-tables)
- [Additional Links](#additional-links)

## Installation

```bash
cargo add sqlitex
```

## Feature showcase
 1. Auto generate method signatures with correct types and
Hover over to see sql code



    ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

(Note: `LazyConnection` has been renamed to `Connection` in newer version. library name was previously called LazySql which has now been renamed to Sqlitex)




2. Compile time errors with good error messages

    ![error_1](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_1.png?raw=true)

    ![error_2](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_2.png?raw=true)

    ![error_3](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_3.png?raw=true)



## Quick Start
```rust
use sqlitex::{Connection, sqlitex};

// Alternatively,
//#[sqlitex("path/to/db.sql")] to point to a .sql file with create table statements.
//#[sqlitex("path/to/existing.db")] to point to an existing database file.
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
```
# Important note on STRICT tables
It is common advice to hear that we should create STRICT tables in sqlite. However, it is recommended not to use it with `sqlitex`

creating STRICT tables in sqlite will make this library less powerful. STRICT table only allows `INT`, `INTEGER`, `REAL`, `TEXT`, `BLOB`, `ANY` datatypes.

This library offers
1. casting as bool
2. creating tables with bool data type,
3. having slightly more flexible data types (e.g. `REAL`, `NUMERIC`, `FLOAT` are all synonymous).

By enabling STRICT tables you will lose all of these features.

[you can read it up more on here](./sqlitex/Documentation.md#a-note-on-strict-tables)
or
[if you are only interested in having compile time checks for boolean using pure sqlite approach](./sqlitex/Documentation.md#how-to-get-boolean-support-for-compile-time-checks-without-using-sqlitexs-bool-or-boolean-data-type)
# Additional Links
For more examples and features, look at the [examples](./examples/) folder and read the [documentations](https://docs.rs/sqlitex/latest/sqlitex/).
