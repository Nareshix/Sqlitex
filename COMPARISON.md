# Comparison

equivalent code comparison can be found below. This is purely a **sqlite** comparison

## Feature Comparison

|                                           | sqlitex                                                                                                                                                                   | rusqlite                | sqlx              |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- | ----------------- |
| Compile-time checks                       | ✅                                                                                                                                                                        | ❌                      | ✅                |
| Speed                                     | Fast by default. Automatically caches all preparred statements. It also applies [optimal pragma settings](./sqlitex/docs_io_readme.md#default-pragma-settings) by default | Fast with configuration | Fast (not tested) |
| Auto type inference                       | ✅                                                                                                                                                                        | ❌ (manual)             | ❌ (manual)       |
| Async support\*                           | ❌                                                                                                                                                                        | ❌                      | ✅                |
| No live DB needed for compile-time checks | ✅                                                                                                                                                                        | —                       | ❌                |
| Row mapping                               | Auto-generated                                                                                                                                                            | Manual                  | Manual            |
| API style                                 | **Declarative**. Runtime features are imperative. Can mix and match both                                                                                                                                                              | Imperative              | Imperative      |
| Bulk operations api                       | ✅ (`_bulk` auto-generated)                                                                                                                                               | ❌                      | ❌                |
| Bool type support                         | ✅                                                                                                                                                                        | ❌ (0/1 manually)       | not tested        |
| postgres `::` for type casting            | ✅                                                                                                                                                                        | ❌                      | ❌                |

\* even though `sqlitex` and `rusqlite` are sync only, you can wrap the calls in `tokio::task::spawn_blocking`.

# Code Comparison

## Sqlitex

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

## Sqlx

Note: you need to set the DATABASE_URL environment variable at build time to get compile time checks for `sqlx`

```rust
use sqlx::SqlitePool;

#[derive(Debug)]
struct User {
    id: i64,
    username: String,
    active: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePool::connect("sqlite::memory:").await?;

    sqlx::query!(
        "
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active BOOLEAN NOT NULL
        )
    "
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "INSERT INTO users (id, username, is_active) VALUES (?, ?, ?)",
        0_i64,
        "Alice",
        true
    )
    .execute(&pool)
    .await?;

    sqlx::query!(
        "INSERT INTO users (id, username, is_active) VALUES (?, ?, ?)",
        1_i64,
        "Bob",
        false
    )
    .execute(&pool)
    .await?;

    let active_users = sqlx::query_as!(
        User,
        "SELECT id, username, is_active as active FROM users WHERE is_active = ?",
        true
    )
    .fetch_all(&pool)
    .await?;

    for user in active_users {
        println!("{}, {}, {}", user.id, user.username, user.active);
    }
    // prints out "0, Alice, true"

    Ok(())
}

```

## Rusqlite

```rust
use rusqlite::{Connection, Result};

#[derive(Debug)]
struct User {
    id: i64,
    username: String,
    active: bool,
}

fn main() -> Result<()> {
    let conn = Connection::open_in_memory()?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY NOT NULL,
            username TEXT NOT NULL,
            is_active INTEGER NOT NULL
        )
    ",
    )?;

    conn.execute(
        "INSERT INTO users (id, username, is_active) VALUES (?1, ?2, ?3)",
        (&0, &"Alice", &1),
    )?;

    conn.execute(
        "INSERT INTO users (id, username, is_active) VALUES (?1, ?2, ?3)",
        (&1, &"Bob", &0),
    )?;

    let mut stmt =
        conn.prepare("SELECT id, username, is_active FROM users WHERE is_active = ?1")?;

    let active_users: Vec<User> = stmt
        .query_map([&1], |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                active: row.get::<_, i64>(2)? != 0,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    for user in active_users {
        println!("{}, {}, {}", user.id, user.username, user.active);
    }

    // prints out "0, Alice, true"

    Ok(())
}

```
