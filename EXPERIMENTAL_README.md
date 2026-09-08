# Sqlitex Architecture & Feature Overview

**Sqlitex** is a lightweight, compile-time verified, synchronous SQLite toolkit for Rust. It is built specifically to provide the type-safety guarantees of tools like SQLx, but without requiring an async runtime, a live database daemon during compilation, or heavy external dependencies.

---

## 1. Core Feature Matrix

### A. Zero-Database Compile-Time Verification
* **Pure Static Analysis:** Unlike SQLx (which requires a live running database or an offline `sqlx-data.json` cache file during compilation), `sqlitex` validates SQL directly against your `.sql` files or `migrations/` directory using an internal SQL parser (`sqlparser` + custom semantic type inference).
* **Instant Recompilation Tracking:** Macro calls embed build-time file watcher tokens (`include_bytes!`). If you modify any migration file or `sqlitex.toml`, the Rust compiler automatically detects the change and re-validates queries without needing `cargo clean`.

### B. Inline Query Macros (`query!` & `query_as!`)
* **`query!("SQL", ...)`**:
  * Runs a query and emits an anonymous record struct (`__Record`) containing named fields matching the SQL columns.
  * Automatically derives `Debug`, `Clone`, and Serde’s `Serialize` / `Deserialize`.
* **`query_as!(TargetType, "SQL", ...)`**:
  * Maps selected columns directly into an existing application struct (e.g. `User { id, name }`), a Rust tuple (e.g. `(i64, String)`), or a scalar primitive (`i64`, `String`).
  * No derive macro required on your target struct as long as its fields match the query projections and implement `FromSql`.

### C. Compile-Time Cardinality Enforcement (Typestate Runners)
Rather than forcing all queries through a generic executor that risks `RowNotFound` or silent multi-row truncation at runtime, `sqlitex` inspects relational constraints (`PRIMARY KEY`, `UNIQUE`, `COUNT(*)`, aggregates) at compile time and emits specialized runner objects:

| Runner | Query Type | Available Methods | Guarantees |
| :--- | :--- | :--- | :--- |
| **`WriteQuery`** | `INSERT`, `UPDATE`, `DELETE`, `CREATE` | `.execute(&conn)` | Returns rows modified (`u64`). Read methods are excluded at compile time. |
| **`ScalarQuery`** | `COUNT(*)`, `SUM`, `AVG` (no `GROUP BY`) | `.fetch_one(&conn)` | Guaranteed $[1, 1]$ rows. Returns raw `T` directly without an unnecessary `Option`. |
| **`OptionalQuery`** | `WHERE id = ?`, `LIMIT 1` | `.fetch_optional(&conn)`, `.fetch_one(&conn)` | Guaranteed $[0, 1]$ rows. Enforces `Option<T>` so missing rows are handled at compile time. |
| **`ManyQuery`** | `SELECT * FROM ...` | `.fetch_all(&conn)`, `.fetch(&conn)`, `.fetch_one()`, `.fetch_optional()` | Bounded $[0, \infty]$. Supports streaming via `Rows` or collecting into a `Vec<T>`. |

### D. Zero-Lock Concurrency & `&self` Ergonomics
* **Stateless Statements:** Queries are prepared on demand and wrapped in RAII drop guards (`PreparredStmt`) that automatically invoke `sqlite3_finalize` upon drop, avoiding statement leaks.
* **Shared Borrowing (`&self`):** All query executions borrow connections immutably (`&Connection`).
  * Eliminates the double-borrow conflict: active row iterators (`Rows`) can coexist with other read operations.
  * Allows sharing connections across OS threads or Tokio blocking pools (`Arc<Connection>`) without requiring an outer `Mutex`.

### E. Native Transaction & Savepoint Support
* Outermost transactions automatically issue `BEGIN IMMEDIATE`.
* Nested transactions automatically issue named savepoints (`SAVEPOINT sqlitex_tx`).
* Rollback guards automatically issue `ROLLBACK` (or rollback to savepoint) if a transaction closure returns an `Err` or encounters an unwinding panic.

### F. Centralized Configuration (`sqlitex.toml`)
Project-wide settings are centralized in the project root:
```toml
[schema]
path = "migrations/" # or "schema.sql"

[database]
path = "app.db"

# [pragmas]
# journal_mode = "WAL"
# synchronous = "NORMAL"
```

### G. Embedded Migrations (`migrate!`)
* Reads migration files (`01_init.sql`, `02_add_users.sql`) in numerical sequence.
* Embeds migration scripts into the compiled binary via `include_str!`, eliminating external directory dependencies in production releases.
* Tracks applied versions and verifies FNV-1a checksums in an internal `_sqlitex_migrations` table to guarantee migration immutability.

### H. Developer CLI Tool (`sqlitex`)
A standalone CLI utility for local development workflow management:
* **`sqlitex init`**: Generates `sqlitex.toml`, the `migrations/` directory, and an initial `01_init.sql`.
* **`sqlitex migrate add <name>`**: Automatically finds the next sequential prefix and creates `migrations/XX_<name>.sql`.

### I. Comprehensive Type & Constraint Support
* **Primitives:** `i64`, `i32`, `f64`, `bool`, `String`, `Option<T>`, `Vec<u8>`.
* **Binary Data:** `BLOB` supported via `Vec<u8>` (reads) and `&[u8]` (bind parameters).
* **Strict Checks:** Validates column count, parameter count, insert omissions, and type affinities directly in the compiler.

---

## 2. Typical Usage Flow

```rust
use serde::{Deserialize, Serialize};
use sqlitex::{Connection, migrate, query, query_as};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_active: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("app.db")?;

    // 1. Run pending migrations embedded from migrations/
    migrate!(&conn)?;

    // 2. Write statement
    let rows_changed = query!(
        "INSERT INTO users (id, username, is_active) VALUES (?, ?, ?)",
        1,
        "alice",
        true
    )
    .execute(&conn)?;
    assert_eq!(rows_changed, 1);

    // 3. Scalar aggregate (Guaranteed 1 row -> returns i64 directly)
    let total_users: i64 = query_as!(i64, "SELECT COUNT(*) FROM users").fetch_one(&conn)?;
    println!("Total registered users: {}", total_users);

    // 4. Unique lookup (Guaranteed <= 1 row -> returns Option<User>)
    let user: Option<User> = query_as!(
        User,
        "SELECT id, username, is_active FROM users WHERE id = ?",
        1
    )
    .fetch_optional(&conn)?;

    // 5. Ad-hoc projection into anonymous Serde-ready record
    let record = query!("SELECT username FROM users WHERE is_active = ?", true)
        .fetch_all(&conn)?;

    for row in record {
        println!("Active user: {}", row.username);
    }

    Ok(())
}
```