use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
use std::time::Instant;

use sqlitex::{Connection as SqlitexConn, sqlitex};

#[sqlitex]
pub struct App {
    init: sql!(
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY NOT NULL, username TEXT NOT NULL)"
    ),
    add_user: sql!("INSERT INTO users (id, username) VALUES (?, ?)"),
    get_user: sql!("SELECT id, username FROM users WHERE id = ?"),
}

struct SqlitexManager {
    db_path: String,
}

impl Manager for SqlitexManager {
    type Type = App;
    type Error = sqlitex::errors::connection::SqliteOpenErrors;

    async fn create(&self) -> Result<App, Self::Error> {
        let conn = SqlitexConn::open(&self.db_path)?;
        Ok(App::new(conn))
    }

    async fn recycle(&self, _: &mut App, _: &Metrics) -> RecycleResult<Self::Error> {
        Ok(())
    }
}

use rusqlite::Connection as RusqliteConn;

struct RusqliteManager {
    db_path: String,
}

impl Manager for RusqliteManager {
    type Type = RusqliteConn;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<RusqliteConn, Self::Error> {
        let conn = RusqliteConn::open(&self.db_path)?;

        conn.execute_batch(
            "
            PRAGMA busy_timeout = 5000;
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
        ",
        )?;
        Ok(conn)
    }

    async fn recycle(&self, _: &mut RusqliteConn, _: &Metrics) -> RecycleResult<Self::Error> {
        Ok(())
    }
}

const NUM_REQUESTS: usize = 1000000;
const POOL_SIZE: usize = 20;

#[tokio::main]
async fn main() {
    let _ = std::fs::remove_file("rusqlite_bench.db");
    let _ = std::fs::remove_file("sqlitex_bench.db");

    println!(
        "Starting benchmark with {} simulated web requests (Pool Size: {})",
        NUM_REQUESTS, POOL_SIZE
    );

    bench_rusqlite().await;
    bench_sqlitex().await;

    let _ = std::fs::remove_file("rusqlite_bench.db");
    let _ = std::fs::remove_file("sqlitex_bench.db");
}

async fn bench_rusqlite() {
    let manager = RusqliteManager {
        db_path: "rusqlite_bench.db".to_string(),
    };

    let pool: Pool<RusqliteManager> = Pool::builder(manager).max_size(POOL_SIZE).build().unwrap();

    {
        let conn = pool.get().await.unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY NOT NULL, username TEXT NOT NULL)",
            [],
        )
        .unwrap();
    }

    let start = Instant::now();
    let mut tasks = Vec::with_capacity(NUM_REQUESTS);

    for i in 0..NUM_REQUESTS {
        let pool = pool.clone();

        tasks.push(tokio::spawn(async move {
            let conn = pool.get().await.unwrap();

            tokio::task::spawn_blocking(move || {
                let mut stmt = conn
                    .prepare_cached("INSERT INTO users (id, username) VALUES (?, ?)")
                    .unwrap();
                stmt.execute(rusqlite::params![i as i64, "Alice"]).unwrap();

                let mut stmt = conn
                    .prepare_cached("SELECT id, username FROM users WHERE id = ?")
                    .unwrap();
                let mut rows = stmt.query(rusqlite::params![i as i64]).unwrap();
                let _row = rows.next().unwrap().unwrap();
            })
            .await
            .unwrap();
        }));
    }

    for task in tasks {
        task.await.unwrap();
    }

    println!("Rusqlite took: {:?}", start.elapsed());
}

async fn bench_sqlitex() {
    let manager = SqlitexManager {
        db_path: "sqlitex_bench.db".to_string(),
    };

    let pool: Pool<SqlitexManager> = Pool::builder(manager).max_size(POOL_SIZE).build().unwrap();

    {
        let mut db = pool.get().await.unwrap();
        db.init().unwrap();
    }

    let start = Instant::now();
    let mut tasks = Vec::with_capacity(NUM_REQUESTS);

    for i in 0..NUM_REQUESTS {
        let pool = pool.clone();

        tasks.push(tokio::spawn(async move {
            let mut db = pool.get().await.unwrap();

            tokio::task::spawn_blocking(move || {
                db.add_user(i as i64, "Alice").unwrap();

                let _user = db.get_user(i as i64).unwrap().unwrap();
            })
            .await
            .unwrap();
        }));
    }

    for task in tasks {
        task.await.unwrap();
    }

    println!("Sqlitex took:  {:?}", start.elapsed());
}

