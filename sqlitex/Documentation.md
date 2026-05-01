# sqlitex

- sqlitex is a sqlite library for rust
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

- `sqlitex` has some nice QOL features like hover over to see sql code and good ide support

  ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

- The type inference system and compile time check also works well for `JOIN`, `CASE` `ctes`, `window function`, `datetime functions` `recursive ctes`, `RETURNING` and more complex scenarios. You can even run `PRAGMA` statements with it.

- Since SQLite defaults to nullable columns, the type inference system defaults to Option<T>. To use concrete types (e.g., String instead of Option<String>), explicitly add **NOT NULL** to your table columns

  For instance,

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

## Features

the `sqlitex!` macro brings `sql!` and `sql_escape_hatch!` macro. so there is no need to import them. and they can only be used within structs defined with `sqlitex!`

Note: Both `sql!` and `sql_escape_hatch!` accept only a single SQL statement at a time. Chaining multiple queries with semicolons (;) is not supported and will result in compile time error.

1. ### `sql!` Macro

   **Always prefer** to use this. It automatically:
   1. **Infers Inputs:** Maps `?` to Rust types (`i64`, `f64`, `String`, `bool`).
   2. **Generates Outputs:** For `SELECT` queries, creates a struct named after the field

2. ### `sql_escape_hatch!` Macro

   #TODO
   - Use this only when you need the sql to to be executed at runtime with some compile time guarantees. **Rarely needed in practice**. You would know when you need it.

   - Originally, `sql_escape_hatch!` is intended more of an escape hatch when you cant use the `sql!` macro due to false positives. False positives are **extremely extremely rare**. Look below for more info. This is why u still have to define structs for SELECT statements and specify types for binding parameters for non-SELECT statements

   #### a. `SELECT`

   You can map a query result to any struct by deriving `SqlMapping`.

   `SqlMapping` maps columns by **index**, not by name. The order of fields in your struct **must** match the order of columns in your `SELECT` statement exactly.

   ```rust
   use sqlitex::{SqlMapping, Connection, sqlitex};

   #[derive(Debug, SqlMapping)]
   pub struct UserStats { // must be pub
       total: i64,      // Maps to column index 0
       status: String,  // Maps to column index 1
   }

   #[sqlitex]
   struct Analytics {
       get_stats: sql_escape_hatch!(
           UserStats, // pass in the struct so you can access the fields later
           "SELECT count(*) as total, status
           FROM users
           WHERE id > ? AND login_count >= ?
           GROUP BY status",
           i64, // Maps to 1st '?'
           i64  // Maps to 2nd '?'
       )
   }

   fn foo{
       let conn = Connection::open_memory()?;
       let mut db = Analytics::new(conn);

       let foo = db.get_stats(100, 5)?;
       for i in foo{
           // i.total and i.status is accessible
       }
   }
   ```

   #### b. No Return Type

   For `INSERT`, `UPDATE`, or `DELETE` statements

   ```rust
   #[sqlitex]
   struct Logger {
       log: sql_escape_hatch!("INSERT INTO logs (msg, level) VALUES (?, ?)", String, i64)
   }
   // can continue to use it normally.
   ```

3. ### Postgres `::` type casting syntax

   ```rust
   sql!("SELECT price::text FROM items")

   // Compiles to:
   // "SELECT CAST(price AS TEXT) FROM items"
   ```

4. ### `all()` and `first()` methods for iterators
   - `all()` collects the iterator into a vector. Just a lightweight wrapper around .collect() to prevent adding type hints (Vec<\_>) in code

     ```rust
     let results = db.get_active_users(false)?;
     let collected_results =results.all()?; // returns a Vec of owned  results from the returned rows
     ```

   - `first()` Returns the first row if available, or None if the query returned no results.

     ```rust
     let results = db.get_active_users(false)?;
     let first_result = results.first()?.unwrap(); // returns the first row from the returned rows
     ```

5. ### Transactions

- Note: you cannot name a field called `transaction` in the struct since its a reserved method name. Failiure to do so will result in a compile time error.
- TODO (link to the transaction example)


## Type Mapping

The tables covers the most common types which are used.

| SQLite Type                                         | Rust Type           |
| --------------------------------------------------- | ------------------- |
| `TEXT`                                              | `String` / `&str`   |
| `INTEGER` / `INT`                                   | `i64`               |
| `REAL` / `FLOAT` / `DOUBLE` / `NUMERIC` / `DECIMAL` | `f64`               |
| `BOOLEAN` / `BOOL`                                  | `bool`              |
| `BLOB`                                              | `Vec<u8>` / `&[u8]` |
| `NULL` (nullable columns)                           | `Option<T>`         |

[All possible type affinities in sqlite is also covered](https://www.sqlite.org/datatype3.html#affinity_name_examples) but it's not recommended to use all of them other than the ones suggested in the table above. Boolean types would look diff in the link because sqlite doesn't natively have them and this library handles it gracefully for us.

## Dynamic runtime features

- **Strongly** recommended to use the `sql!` macro for most use-cases. Dynamic runtime features are only needed in **rare** scenarios.

### Runtime Features

- Dynamic runtime features happens fully at runtime. All the features are stated below in this code block.

TODO link to the runtime example

### Transactions at Runtime
TODO link to the transaction_runtime example

## Notes

### Strict INSERT Validation

- Although standard SQL allows inserting any number of columns to a table, sqlitex checks INSERT statements at compile time. If you omit any column (except for `AUTOINCREMENT` and `DEFAULT`), code will fail to compile. This means you must either specify all columns explicitly, or use implicit insertion for all columns. This is done to prevent certain runtime errors such as `NOT NULL constraint failed` and more.

### - Valid SQL syntax or type inference fails at compile-time?

- I tried my best to support as many sql and sqlite-specific queries as possible.

- This isnt naturally easy in sqlite as they dont provide any api to give us type inference and schema awareness validation.

- In the extremely rare case of a False positives (valid SQL syntax **fails** or type inference **incorrectly fails**), you can fall back to the `sql_escape_hatch!` macro. Would appreciate it if you could open an issue as well.

