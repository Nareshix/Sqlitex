testing is needed since i renamed everything. lazysql still works and the prev docs is in the crates.io page for now.
# sqlitex

- sqlitex is a sqlite library for rust
- Has compile time guarantees
- Ergonomic
- Fast. Automatically caches and reuses prepared statements for you
- Some downsides that may or may not be fixed in future
  1. it follows an opinionated API design
  2. Doesn't support Batch Execution ergonomically. You would need to resort to `sql!()` or `sql_runtime!()` macro

# Overview

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Connection methods](#connection-methods)
  1. [Inline Schema](#1-inline-schema)
  2. [SQL File](#2-sql-file)
  3. [Live Database](#3-live-database)
- [Features](#features)
  1. [`sql!` Macro](#sql-macro)
  2. [`sql_runtime!` Macro](#sql_runtime-macro)
     - [SELECT](#1-select)
     - [INSERT, UPDATE, DELETE etc.](#2-no-return-type)
  3. [postgres `::` syntax](#postgres--type-casting-syntax)
  4. [`all()` and `first()` methods for iterators](#all-and-first-methods-for-iterators)
  5. [Transactions](#transactions)

- [Dynamic runtime features](#dynamic-runtime-features)
  1. [How is this different from `sql_runtime!`](#how-is-this-different-from--sql_runtime)
  2. [Runtime Features](#runtime-features)
  3. [Transactions at Runtime](#transactions-at-runtime)
- [Type Mapping](#type-mapping)
- [Notes](#notes)
  1. [Strict INSERT Validation](#strict-insert-validation)
  2. [False positives during compile time checks](#false-positive-during-compile-time-checks)
  3. [Cannot type cast as Boolean](#cannot-type-cast-as-boolean)
- [TODOS](#todos)

## Installation

Run the following Cargo command in your project directory:

```bash
cargo add sqlitex
```

OR

Go to [sqlitex's crates.io](https://crates.io/crates/sqlitex) to get the latest version. Add that to following line to your Cargo.toml:

```toml
sqlitex = "*" # Replace the "*" with the latest version
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
            is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)) -- the library infers this as bool. more info below
        )
    "),

    // postgres `::` type casting is supported. Alternatively u can use CAST AS syntax
    add_user: sql!("INSERT INTO users (id, username, is_active) VALUES (?::real, ?, ?)"),

    get_active_users: sql!("SELECT id::real, username, is_active as active FROM users WHERE is_active = ?"),
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
        // Respects Aliases (is_active -> active)
        let user = user?;
        println!("{}, {}, {}", user.active, user.username, user.id); // note user.id is float as we type casted it in the sql stmt
    }

    Ok(())
    // prints out "true, Alice, 0"
}
```

---

- `sqlitex` has some nice QOL features like hover over to see sql code and good ide support

  ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

- The type inference system and compile time check also works well for `JOIN`, `CASE` `ctes`, `window function`, `datetime functions` `recursive ctes`, `RETURNING` and more complex scenarios. You can even run `PRAGMA` statements with it.

- Since SQLite defaults to nullable columns, the type inference system defaults to Option<T>. To use concrete types (e.g., String instead of Option<String>), explicitly add **NOT NULL** to your table columns

- You cannot name a field called `transaction` in the struct since its a reserved method name for transactions. Failiure to do so will result in a compile time error.

- There will be rare scenarios when a type is impossible to infer. `sqlitex` will tell you specifically which binding parameter or expression cannot be inferred and will suggest using type casting via PostgreSQL's `::` operator or standard SQL's `CAST AS`. Note that you can't type cast as `boolean` for now.

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

Note: for method 2 and 3, you can technically CREATE TABLE as well but to ensure that they are taken into consideration for compile time check, add them at the top of your struct

## Features

the `sqlitex!` macro brings `sql!` and `sql_runtime!` macro. so there is no need to import them. and they can only be used within structs defined with `sqlitex!`

Note: Both `sql!` and `sql_runtime!` accept only a single SQL statement at a time. Chaining multiple queries with semicolons (;) is not supported and will result in compile time error.

1. ### `sql!` Macro

   Always prefer to use this. It automatically:
   1. **Infers Inputs:** Maps `?` to Rust types (`i64`, `f64`, `String`, `bool`).
   2. **Generates Outputs:** For `SELECT` queries, creates a struct named after the field

2. ### `sql_runtime!` Macro
   - Use this only when you need the sql to to be executed at runtime with some compile time guarantees. **Rarely needed in practice**. You would know when you need it.

   - Originally, `sql_runtime!` is intended more of an escape hatch when you cant use the `sql!` macro due to false positives. False positives are **extremely extremely rare**. Look below for more info. This is why u still have to define structs for SELECT statements and specify types for binding parameters for non-SELECT statements

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
       get_stats: sql_runtime!(
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
       log: sql_runtime!("INSERT INTO logs (msg, level) VALUES (?, ?)", String, i64)
   }
   // can continue to use it normally.
   ```

3. ### Postgres `::` type casting syntax

   Note: bool type casting is not supported for now

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

  ```rust
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
  ```

## Type Mapping

| SQLite Context | Rust Type         | Notes                                                                                                                                                                                                                                       |
| :------------- | :---------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `TEXT`         | `String` / `&str` | -                                                                                                                                                                                                                                           |
| `INTEGER`      | `i64`             | -                                                                                                                                                                                                                                           |
| `REAL`         | `f64`             | Includes `FLOAT`, `DOUBLE`                                                                                                                                                                                                                  |
| `BOOLEAN`      | `bool`            | Requires `CHECK (col IN (0,1))` or `Check (col = 0 OR col = 1)`. You could technically use `BOOL` or `BOOLEAN` as the data type when creating table (due to sqlite flexible type nature) and it would work as well. But this is discouraged |
| Nullable       | `Option<T>`       | When a column or expr has a possibility of returning `NULL`, this will be returned. its recommended to use `NOT NULL` when creating tables so that ergonomic-wise you don't always have to use Some(T) when adding parameters               |

## Dynamic runtime features

- **Strongly** recommended to use the `sql!` macro for most use-cases. Dynamic runtime features are only needed in **rare** scenarios.

### How is this different from `sql_runtime!`

- `sql_runtime!` is intended more of an escape hatch when you cant use the `sql!` macro due to false positives. False positives are **extremely extremely rare**. Look below for more info. This is why u still have to define structs for SELECT statements and specify types for binding parameters for non-SELECT statements

### Runtime Features

- Dynamic runtime features happens fully at runtime. All the features are stated below in this code block.

  ```rust

  use sqlitex::Connection;

  fn main() -> Result<(), Box<dyn std::error::Error>> {
      let conn = Connection::open_memory()?;

      // Use execute_dynamic for write statements (CREATE, INSERT, UPDATE, DELETE, etc.)
      conn.execute_dynamic(
          "CREATE TABLE products (
              id INTEGER PRIMARY KEY,
              name TEXT NOT NULL,
              price REAL,
              in_stock INTEGER
          )",
      )?;

      // _rows_affected variable is the number of rows modified, which in this case is an insert of 3 rows
      let _rows_affected = conn.execute_dynamic(
          "INSERT INTO products (name, price, in_stock) VALUES
          ('Laptop', 999.99, 1),
          ('Mouse', 25.50, 1),
          ('Keyboard', 75.00, 0)",
      )?;

      // Use query_dynamic for running SELECT statements
      let results = conn.query_dynamic("SELECT * FROM products")?;
      println!("Headers: {:?}", results.column_names); // id, name, price, in_stock

      // row_result is an iterator
      for row_result in results {
          let row = row_result?;
          for value in row {
              print!("{:?} ", value); // or u could do value.as_string(), value.as_f64(), value.as_i64(), etc. to convert the enum to specific type
          }
      }

      // u can use helper functions like first() or all() to get a vector of rows.
      let _first_row = conn
          .query_dynamic("SELECT name, price FROM products WHERE id = 1")?
          .first()?; // or .all()? for all rows

      Ok(())
  }

  ```

### Transactions at Runtime

```rust
use sqlitex::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;

    conn.execute_dynamic("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT UNIQUE)")?;

    // Successful Transaction
    let user_count = conn.transaction(|tx| {
        tx.execute_dynamic("INSERT INTO users (name) VALUES ('Alice')")?;
        tx.execute_dynamic("INSERT INTO users (name) VALUES ('Bob')")?;

        let row = tx
            .query_dynamic("SELECT COUNT(*) FROM users")?
            .first()?
            .unwrap();
        Ok(row[0].as_i32()) // Return the count
    })?;

    println!("{}", user_count); // Prints 2

    // 3. Failed Transaction (Automatic Rollback)
    // We try to add Charlie, then Alice again (who already exists).
    let result = conn.transaction(|tx| {
        tx.execute_dynamic("INSERT INTO users (name) VALUES ('Charlie')")?; // Succeeds
        tx.execute_dynamic("INSERT INTO users (name) VALUES ('Alice')")?; // Fails (UNIQUE constraint)
        Ok(())
    });

    if let Err(e) = result {
        println!("{}", e);
    }

    // Charlie should NOT exist in the DB because the transaction reverted.
    let final_count = conn
        .query_dynamic("SELECT COUNT(*) FROM users")?
        .first()?
        .unwrap()[0]
        .as_i32();

    println!("Charlie not added. Total count: {}", final_count); // prints 2 since Charlie was not added.

    Ok(())
}

```

## Notes

### Strict INSERT Validation

- Although standard SQL allows inserting any number of columns to a table, sqlitex checks INSERT statements at compile time. If you omit any column (except for `AUTOINCREMENT` and `DEFAULT`), code will fail to compile. This means you must either specify all columns explicitly, or use implicit insertion for all columns. This is done to prevent certain runtime errors such as `NOT NULL constraint failed` and more.

### False positives during compile time checks

- I tried my best to support as many sql and sqlite-specific queries as possible.

- This isnt naturally easy in sqlite as they dont provide any api to give us type inference and schema awareness validation.

- In the extremely rare case of a False positives (valid SQL syntax **fails** or type inference **incorrectly fails**), you can fall back to the `sql_runtime!` macro. Would appreciate it if you could open an issue as well.

### Cannot type cast as Boolean

- This is a limitation of sqlite since it doesn't natively have `boolean` type. I may find some workaround in the future but it's not guaranteed. For now if you want to type cast as bool, u have to type cast it as an `integer` and add either 1 (`TRUE`) or 0 (`False`)

## TODOS

1. [upsert](https://www.cockroachlabs.com/blog/sql-upsert/)
2. check_constarint field in SELECT is ignored for now. maybe in future will make use of this field
3. cant cast as bool

4. bulk insert
5. begin immediate
6. chrono/time/jiff or other datetime-based library support
7. better egonomic for bulk operation? maybe.
8. url crate?

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
