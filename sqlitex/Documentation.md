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

- [Type Mapping](#type-mapping)
- [When to use `sql_escape_hatch!`](#when-to-use-sql_escape_hatch)
    - [How to use `sql_escape_hatch!`](#how-to-use-sql_escape_hatch)
         - [SELECT statements](#a-select-statements)
         - [No Return Type](#b-no-return-type)
- [Miscs](#miscs)
    - [Strict INSERT Validation](#strict-insert-validation)

## Installation

```bash
cargo add sqlitex
```

## Quick Start
For more examples and features, look at the [examples](./examples/) folder and read the [documentations](https://docs.rs/sqlitex/latest/sqlitex/).
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

    // you don't have to import sql! macro. #[sqlitex] brings with it
    init: sql!("
    -- Note the NOT NULL constraints which allows us to use concrete types instead of Option<T>, (e.g., `i32` instead of `Option<i32>`)
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active BOOL NOT NULL
        )
    "),

    //`sql!` accept only a single SQL statement at a time.
    // Chaining multiple queries with semicolons (;) is not supported
    //and will result in `EOF error` during compile time.

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
   2. **Generates Outputs:** For `SELECT` queries, creates a struct named after the field

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

## When to use `sql_escape_hatch!`

you will most likely **never** need to use this.

For some context, sqlite does not expose any api for type inference and schema awareness validation. Hence, I had to build a custom sql parser and implement type inference and schema awareness myself in order to provide compile time guarantees.

In theory, there might be some edge cases for **extremely complex sql queries** that I might have missed, meaning the sql query should work perfectly fine in runtime but the compile time checks fail.

In practice however, most SQL queries are straightforward enough that one will ***almost never*** get close to hitting it.

It is also important to calrify that there will **never** be a case when a sql query passes compile time check but fails at runtime. If it compiles, it works.

This might sound like a perfect candidate for sql runtime features. While you can perfectly use it for this use case, u will miss out on the compile time guarantees.
Since the sql is correct but compiler fails to catch it, u can use `sql_escape_hatch!` to define the sql  itself. The code would seem abit more verbose but u can still secure that compile time guarantees.

If you do somehow encounter this *false positive*, I would really appreicate it if you could open an issue on the [github repo](https://github.com/nareshix/sqlitex/issues).

### How to use `sql_escape_hatch!`

   #### a. `SELECT` statements

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

## Miscs

### Strict INSERT Validation

- Although standard SQL allows inserting any number of columns to a table, sqlitex checks INSERT statements at compile time. If you omit any column (except for `AUTOINCREMENT` and `DEFAULT`), code will fail to compile. This means you must either specify all columns explicitly, or use implicit insertion for all columns. This is done to prevent certain runtime errors such as `NOT NULL constraint failed` and more.
