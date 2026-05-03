//! Note that FTS5 does not have native compile time checks.
//! You would need to fall back to runtime features.
//! Might support compile time checks for FTS5 in future
use sqlitex::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;

    // FTS5 virtual table. Must use runtime since compile time doesn't support virtual tables
    conn.execute_batch("CREATE VIRTUAL TABLE articles USING fts5(title, body)")?;

    // Insert some data
    conn.execute(
        "INSERT INTO articles VALUES ('SQLite is great', 'SQLite is a lightweight database')",
    )?;
    conn.execute(
        "INSERT INTO articles VALUES ('Rust is fast', 'Rust is a systems programming language')",
    )?;
    conn.execute("INSERT INTO articles VALUES ('Full text search', 'FTS5 allows powerful text searching in SQLite')")?;

    // Full text search
    let results = conn.query("SELECT title, body FROM articles WHERE articles MATCH 'SQLite'")?;

    println!("Search results for 'SQLite':");
    for row in results {
        let row = row?;
        println!("  Title: {}", row[0].as_string());
        println!("  Body:  {}", row[1].as_string());
        println!();
    }

    let results =
        conn.query("SELECT title, rank FROM articles WHERE articles MATCH 'SQLite' ORDER BY rank")?;

    println!("Ranked results:");
    for row in results {
        let row = row?;
        println!("  {} (rank: {})", row[0].as_string(), row[1].as_f64());
    }

    // Phrase search
    let results =
        conn.query("SELECT title FROM articles WHERE articles MATCH '\"text search\"'")?;

    println!("Phrase search for 'text search':");
    for row in results {
        let row = row?;
        println!("  {}", row[0].as_string());
    }

    // Prefix search
    let results = conn.query("SELECT title FROM articles WHERE articles MATCH 'Rust*'")?;

    println!("Prefix search for 'Rust*':");
    for row in results {
        let row = row?;
        println!("  {}", row[0].as_string());
    }

    Ok(())
}
