# Sqlitex

Sqlitex is a sqlite library for rust which aims to be simple and powerful. It offers

- Compile time guarantees
- Ergonomic with excellent IDE support
- Very Fast
  - Automatically caches and reuses prepared statements for you
  - Automatically applies optimal PRAGMA settings for performance and reliability

For more details look at the [github repo](https://github.com/Nareshix/sqlitex) or read the [docs](http://docs.rs/sqlitex/latest)

## Quickstart

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