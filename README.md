# Sqlitex

Sqlitex is a sqlite library for rust which aims to be simple and powerful. It offers

- Compile time guarantees
- Ergonomic with excellent IDE support
- Very Fast
  - Automatically caches and reuses prepared statements for you
  - Automatically applies optimal PRAGMA settings for performance and reliability

- [Quickstart](#quickstart)
- [Feature Showcase](#feature-showcase)
- [Comparison with other libraries](#comparison-with-other-libraries)

## Quickstart
Install it via

```bash
cargo add sqlitex
```

Simple usage example:

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

_A more detailed version of this exact quickstart can be found_ [here](./examples/quick_start.rs)

For more examples and features, look at the [examples](./examples/) folder or read the [documentations](https://docs.rs/sqlitex/latest/sqlitex/).


## Feature showcase

1.  Auto generate method signatures with correct types and
    Hover over to see sql code

    ![usage](https://github.com/Nareshix/sqlitex/raw/main/amedia_for_readme/usage.gif)

(Note: `LazyConnection` has been renamed to `Connection` in newer version. library name was previously called LazySql which has now been renamed to Sqlitex)

2. Compile time errors with good error messages

   ![error_1](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_1.png?raw=true)

   ![error_2](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_2.png?raw=true)

   ![error_3](https://github.com/Nareshix/sqlitex/blob/main/amedia_for_readme/error_3.png?raw=true)



## Comparison with other libraries
[Look here](./COMPARISON.md)
