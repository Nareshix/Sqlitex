# Sqlitex

Sqlitex is a sqlite library for rust with compile time guarantees. It also has additional features:

- Ergonomic with excellent IDE support
- Very Fast
  - Automatically caches and reuses prepared statements for you
  - Automatically applies optimal PRAGMA settings for performance and reliability (e.g., WAL, synchronous=NORMAL and more).

# Overview

- [Installation](#installation)
- [Feature showcase](#feature-showcase)
- [Quick Start](#quick-start)
- [Important note on STRICT tables](#important-note-on-strict-tables)

## Installation

```bash
cargo add sqlitex
```

## Feature showcase

1.  Auto generate method signatures with correct types and
    Hover over to see sql code

    ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

(Note: `LazyConnection` has been renamed to `Connection` in newer version. library name was previously called LazySql which has now been renamed to Sqlitex)

2. Compile time errors with good error messages

   ![error_1](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_1.png?raw=true)

   ![error_2](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_2.png?raw=true)

   ![error_3](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_3.png?raw=true)

## Quick Start
For more examples and features, look at the [examples](./examples/) folder or read the [documentations](https://docs.rs/sqlitex/latest/sqlitex/).


```rust
use sqlitex::{Connection, sqlitex};

#[sqlitex]
struct App {
    init: sql!("
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active BOOL NOT NULL
        )
    "),

    add_user: sql!("INSERT INTO users (id, username, is_active) VALUES (?, ?, ?);"),

    get_active_users: sql!("SELECT id, username, is_active as active FROM users WHERE is_active = ?"),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;
    let mut db = App::new(conn);

    db.init()?;
    db.add_user(0, "Alice", true)?;
    db.add_user(1, "Bob", false)?;

    let active_users = db.get_active_users(true)?;

    for user in active_users {
        let user = user?;
        println!("{}, {}, {}", user.id, user.username, user.active);
    }

    Ok(())
    // prints out "0, Alice, true"
}
```
*A more detailed version of this exact quickstart can be found* [here](./examples/quick_start.rs)


# Important note on STRICT tables

It is a common advice to create STRICT tables in sqlite. However, it is recommended not to use it with `sqlitex`

creating STRICT tables in sqlite will make this library less powerful. STRICT table only allows `INT`, `INTEGER`, `REAL`, `TEXT`, `BLOB`, `ANY` datatypes.

This library offers

1. casting as bool
2. creating tables with bool data type,
3. having slightly more flexible data types (e.g. `REAL`, `NUMERIC`, `FLOAT` are all synonymous).

By enabling STRICT tables you will lose all of these features.

[you can read it up more on here](./sqlitex/Documentation.md#a-note-on-strict-tables)
or
[if you are only interested in having compile time checks for boolean using pure sqlite approach](./sqlitex/Documentation.md#how-to-get-boolean-support-for-compile-time-checks-without-using-sqlitexs-bool-or-boolean-data-type)