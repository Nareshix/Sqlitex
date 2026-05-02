# sqlitex

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Connection methods](#connection-methods)
  1. [Inline Schema](#1-inline-schema)
  2. [SQL File](#2-sql-file)
  3. [Live Database](#3-live-database)
- [Sqlitex features](#sqlitex-features)
  1. [postgres `::` syntax](#postgres--type-casting-syntax)
  2. [`all()` and `first()` methods for iterators](#all-and-first-methods-for-iterators)
  3. [Transactions](#transactions)
  4. [Runtime Features](#runtime-features)
  5. [BLOB](#blob)

- [Important note on STRICT tables](#important-note-on-strict-tables)
- [When to use `sql_escape_hatch!`](#when-to-use-sql_escape_hatch)
  - [How to use `sql_escape_hatch!`](#how-to-use-sql_escape_hatch)
    - [SELECT statements](#a-select-statements)
    - [No Return Type](#b-no-return-type)

- [Type casting](#type-casting)
- [Strict INSERT Validation](#strict-insert-validation)

## Installation

```bash
cargo add sqlitex
```

## Quick Start

For more examples, look at the [examples folder in github](https://github.com/Nareshix/sqlitex/tree/main/examples)

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

If you use IDE extensions such as rust-analyser and it does not pick up changes like showing old errors, you may have to type anything on that rust file (e.g. spacebar) to immediately trigger the ide extension for it to pick up the changes in the sql file.

![sql-file-watcher-trigger](https://raw.githubusercontent.com/Nareshix/sqlitex/refs/heads/main/amedia_for_readme/sql-file-watcher-trigger.gif)

If it still does not work, you may have to restart ur rust lsp server.

This issue can be avoided in the future when [tracked_path](https://github.com/rust-lang/rust/issues/99515) gets stabilised

### 3. Live Database

Point to an existing `.db` binary file. `sqlitex` inspects the live metadata to validate your queries.

```rust
#[sqlitex("production_snapshot.db")]
struct App { ... }
```

## Sqlitex features

the `#[sqlitex!]` macro brings `sql!` and `sql_escape_hatch!` macro. so there is no need to import them. and they can only be used within structs defined with `sqlitex!`

Note: Both `sql!` and `sql_escape_hatch!` accept only a single SQL statement at a time. Chaining multiple queries with semicolons (;) is not supported and will result in compile time error.

1. ### `sql!` Macro

   **Always prefer** to use this. It automatically:
   1. **Infers Inputs:** Maps `?` to Rust types (`i64`, `f64`, `String`, `bool`).
   2. **Generates Outputs:** For `SELECT` / `RETURNING` queries, creates a struct named after the field

2. ### `sql_escape_hatch!` Macro
   You will almost never need to use this in practice. If you are curious on wht it does, read [on this section below](#when-to-use-sql_escape_hatch)

## Postgres `::` type casting syntax

```rust
sql!("SELECT price::text FROM items")

// Compiles to:
// "SELECT CAST(price AS TEXT) FROM items"
```

## `all()` and `first()` methods for iterators

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

## Transactions

[Read this simple and short example on how to use transactions (at compile time)](https://github.com/Nareshix/sqlitex/blob/main/examples/transactions.rs)

## Runtime features

**Strongly** recommended to use the `sql!` macro when possible

[simple and short example for all runtime feature](https://github.com/Nareshix/sqlitex/blob/main/examples/runtime.rs)

[simple and short example for transaction feature at runtime ](https://github.com/Nareshix/sqlitex/blob/main/examples/transactions_runtime.rs)

## BLOB

[simple and short example for BLOB](https://github.com/Nareshix/sqlitex/tree/main/examples/blob)

## Important note on STRICT tables

It is a common advice to create STRICT tables in sqlite. However, it is recommended not to use it with `sqlitex`

when using `sqlitex`, it **automatically** uses its own built-in "STRICT" table, which is more flexible and much more powerful than sqlite's native STRICT tables. It mostly follows [sqlite type affinity](https://www.sqlite.org/datatype3.html#affinity_name_examples) except for how `BOOLEAN`/`BOOL` is handled. It is shown in the table below

| SQLite Type without STRICT TABLE                                                                         | Rust Type           |
| -------------------------------------------------------------------------------------------------------- | ------------------- |
| `TEXT` / `CHARACTER` / `VARCHAR` / `CHARVARYING` / `CHARACTERVARYING` / `NVARCHAR` / `CLOB`              | `String` / `&str`   |
| `INTEGER` / `INT` / `TINYINT` / `SMALLINT` / `MEDIUMINT` / `BIGINT` / `BIGINTUNSIGNED` / `INT2` / `INT8` | `i64`               |
| `REAL` / `DOUBLE` / `DOUBLEPRECISION` / `FLOAT` / `NUMERIC` / `DECIMAL`                                  | `f64`               |
| **`BOOLEAN`** / **`BOOL`**                                                                               | `bool`              |
| `BLOB` / `BYTEA`                                                                                         | `Vec<u8>` / `&[u8]` |

| SQLite Type with STRICT TABLE | Rust Type           |
| ----------------------------- | ------------------- |
| `INTEGER` / `INT`             | `i64`               |
| `REAL`                        | `f64`               |
| `TEXT`                        | `String` / `&str`   |
| `BLOB`                        | `Vec<u8>` / `&[u8]` |
| `ANY`                         | `-`                 |

As we can see, `sqlitex` built-in "STRICT" table gives us more flexible types like FLOAT and DECIMAL and, more powerfully, a Boolean datatype

This would also affect how casting works. Using the built in "STRICT" table for instance, allows casting with bool, but manually creating a STRICT table will make casting as bool impossible.

Internally, creating table with `BOOLEAN` and `BOOL` data type aliases to `INTEGER CHECK (col IN (0, 1))`

### How to get boolean support for compile time checks without using `sqlitex`'s `bool` or `boolean` data type?

If u prefer a sqlite-pure approach, make sure u add a check constraint for the column like either one of the following and sqlitex will automatically detect it as bool:

1.  CHECK (col IN (0, 1))
2.  CHECK (col = 0 OR col = 1)

It does not matter whether the table is created with STRICT or not. You can still get compile time checks and boolean support as long as you have either of the check constraint.

For isntance

```rust
#[lazy_sql]
struct AppDatabase {
    init: sql!("
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)) -- the library infers this as bool.
        )
    "),
    // ...
}
```

## When to use `sql_escape_hatch!`

you will most likely **never** need to use this.

For some context, sqlite does not expose any api for type inference and schema awareness validation. Hence, I had to build a custom sql parser and implement type inference and schema awareness myself in order to provide compile time guarantees.

In theory, there might be some edge cases for **extremely complex sql queries** that I might have missed, meaning the sql query should work perfectly fine in runtime but the compile time checks fail. In practice however, most SQL queries are straightforward enough that one will **_almost never_** get close to hitting it. It is also important to calrify that there will **never** be a case when a sql query passes compile time check but fails at runtime. If it compiles, it works.

This might sound like a perfect candidate for sql runtime features. While you can perfectly use it for this use case, u will miss out on the compile time guarantees.
Since the sql is correct but compiler fails to catch it, u can use `sql_escape_hatch!` to define the sql itself. The code would seem abit more verbose but u can still secure that compile time guarantees.

If you do somehow encounter this _false positive_, I would really appreicate it if you could open an issue on the [github repo](https://github.com/nareshix/sqlitex/issues).

### How to use `sql_escape_hatch!`

#### a. `SELECT` statements
Note: This also works for `INSERT... RETURNING`
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

## Type casting
only these are supported for now to avoid unexpected behaviour.

    Integer -> Real
    Real -> Integer (note it gets truncated)
    Integer -> Text
    Real -> Text
    Bool -> Integer (true -> 1, false -> 0)
    Bool -> Real (true -> 1.0, false -> 0.0)

## Strict INSERT Validation

- Although standard SQL allows inserting any number of columns to a table, sqlitex checks INSERT statements at compile time. If you omit any column (except for `AUTOINCREMENT` and `DEFAULT`), code will fail to compile. This means you must either specify all columns explicitly, or use implicit insertion for all columns. This is done to prevent certain runtime errors such as `NOT NULL constraint failed` and more.
