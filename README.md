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

#[sqlitex]
struct AppDatabase {
    // all create tables must be at the top before read/write logic in order to get compile time checks

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

- `sqlitex` has some nice QOL features like hover over to see sql code and good ide support (note: `LazyConnection` has been renamed to `Connection` in newer version)

  ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

- The type inference system and compile time check also works well for `JOIN`, `CASE` `CTEs`, `window function`, `datetime functions` `recursive ctes`, `RETURNING` and more complex scenarios. You can even run `PRAGMA` statements with it.

- Since SQLite defaults to nullable columns, the type inference system defaults to `Option<T>`. To use concrete types (e.g., `i32` instead of `Option<i32>`), explicitly add **NOT NULL** to your table schema.




- Some examples of compile time errors

  ![error_1](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_1.png?raw=true)

  ![error_2](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_2.png?raw=true)

## Connection methods

`sqlitex` supports 3 ways to define your schema, depending on your workflow.

### 1. Inline Schema

As seen in the Quick Start. Define tables inside the struct.

```rust
#[sqlitex]
struct App { ... }
```

### 2. SQL File

Point to a `.sql` file. The compile time checks will be done against this sql file (ensure that there is `CREATE TABLE`). `sqlitex` watches this file; if you edit it, rust recompiles automatically to ensure type safety.

```rust
#[sqlitex("schema.sql")]
// you dont have to create tables. Any read/write sql queries gets compile time guarantees.
struct App { ... }
```

### 3. Live Database

Point to an existing `.db` binary file. `sqlitex` inspects the live metadata to validate your queries.

```rust
#[sqlitex("production_snapshot.db")]
struct App { ... }
```

## Type Mapping

The tables covers the most common types which are used.

| SQLite Type                  | Rust Type           |
|-----------------------------|---------------------|
| `TEXT`                      | `String` / `&str`   |
| `INTEGER` / `INT`           | `i64`               |
| `REAL` / `FLOAT` / `DOUBLE` / `NUMERIC` / `DECIMAL` | `f64`               |
| `BOOLEAN` / `BOOL`          | `bool`              |
| `BLOB`                      | `Vec<u8>` / `&[u8]` |
| `NULL` (nullable columns)   | `Option<T>`         |



## TODOS
1. rn blob loads everything to memory. add streaming support for blob

2. check_constarint field in SELECT is ignored for now. maybe in future will make use of this field
nutype/nnn support basic
upsert, INSERT OR REPLACE INTO users (id, name) VALUES (?, ?)

4. bulk insert
5. begin immediate
6. chrono/time/jiff or other datetime-based library support
7. better egonomic for bulk operation? maybe.
8. url crate?
  1. it follows an opinionated API design
  2. Doesn't support Batch Execution ergonomically. You would need to resort to `sql!()` or `sql_escape_hatch!()` macro

show how blob is used in READEME
//TODO sqlite3_busy_timeout does return an int. It is nearly a gurantee for this
// function to never fail. but its still good to handle it. If it fails mean
// the sql query is taking more than 5 second which means its inefficent lol
hence give eoption to change the timeout
make the readme shorter

in case CREATE TABLE is done after a random query in sql_struct should i allow it? like scan whole struct first instead of top down? at least show a warning

e.g. blob support to add later

```rust
use std::fs;
use sqlitex::{Connection, sqlitex};

#[sqlitex]
struct AppDatabase {
    init: sql!("
        CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            payload BLOB NOT NULL
        )
    "),
    insert_doc: sql!("INSERT INTO documents (id, name, payload) VALUES (?, ?, ?)"),
    get_doc: sql!("SELECT id, name, payload FROM documents WHERE id = ?"),


}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("asd.db")?;
    let mut db = AppDatabase::new(conn);
    db.init()?;

    // 1. Read the image file from disk into a Vec<u8>
    let image_bytes = fs::read("error_1.png")?;
    println!("Read image from disk: {} bytes", image_bytes.len());

    // 2. Insert into the database (pass a reference to the Vec so it becomes &[u8])
    db.insert_doc(2, "error_1.png", &image_bytes)?;
    println!("Image successfully saved to SQLite!");

    // 3. Retrieve the image back from the database
    let results = db.get_doc(2)?;
    let doc = results.first()?.unwrap();
    println!("Retrieved document '{}' with {} bytes.", doc.name, doc.payload.len());

    // 4. Write it back to the disk with a new name to verify!
    fs::write("restored_error_1.png", &doc.payload)?;
    println!("Image restored to disk as 'restored_error_1.png'. Open it to see!");

    Ok(())
}
```
strict table can break certain features like bool datatype