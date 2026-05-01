# Sqlitex
- Sqlitex is a sqlite library for rust
- Has compile time guarantees
- Ergonomic
- Fast. Automatically caches and reuses prepared statements for you

# Overview

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Connection methods](#connection-methods)
  1. [Inline Schema](#1-inline-schema)
  2. [SQL File](#2-sql-file)
  3. [Live Database](#3-live-database)
- [Features](#features)
  1. [`sql!` Macro](#sql-macro)
  2. [`sql_escape_hatch!` Macro](#sql_escape_hatch-macro)
     - [SELECT](#1-select)
     - [INSERT, UPDATE, DELETE etc.](#2-no-return-type)
  3. [postgres `::` syntax](#postgres--type-casting-syntax)
  4. [`all()` and `first()` methods for iterators](#all-and-first-methods-for-iterators)
  5. [Transactions](#transactions)

- [Dynamic runtime features](#dynamic-runtime-features)
  1. [How is this different from `sql_escape_hatch!`](#how-is-this-different-from--sql_escape_hatch)
  2. [Runtime Features](#runtime-features)
  3. [Transactions at Runtime](#transactions-at-runtime)
- [Type Mapping](#type-mapping)
- [Notes](#notes)
  1. [Strict INSERT Validation](#strict-insert-validation)
  2. [False positives during compile time checks](#false-positive-during-compile-time-checks)
  3. [Cannot type cast as Boolean](#cannot-type-cast-as-boolean)
- [TODOS](#todos)

## Installation

```bash
cargo add sqlitex
```


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

    // you don't have to import sql! macro. sqlitex brings with it
    init: sql!("
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active BOOL NOT NULL
        )
    "),

    // postgres `::` type casting is supported. Alternatively u can use `CAST AS` syntax
    add_user: sql!("INSERT INTO users (id, username, is_active) VALUES (?::REAL, ?, ?)"),

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

    // active_users is an iterator
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

---

- `sqlitex` has some nice QOL features like hover over to see sql code and good ide support (note: `LazyConnection` has been renamed to `Connection` in newer version. library name was previously called LazySql which has been renamed to Sqlitex)

  ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

- The type inference system and compile time check also works well for `JOIN`, `CASE` `CTEs`, `window function`, `datetime functions` `recursive ctes`, `RETURNING` and more complex scenarios. You can even run `PRAGMA` statements with it.

- Since SQLite defaults to nullable columns, the type inference system defaults to `Option<T>`. To use concrete types (e.g., `i32` instead of `Option<i32>`), explicitly add **NOT NULL** to your table schema.




- Some examples of compile time errors

  ![error_1](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_1.png?raw=true)

  ![error_2](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_2.png?raw=true)

  ![error_3](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_3.png?raw=true)

