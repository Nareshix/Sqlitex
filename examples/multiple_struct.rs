use sqlitex::{Connection, sqlitex};

#[sqlitex]
struct UsersDb {
    init: sql!(
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL)"
    ),
    add: sql!("INSERT INTO users (name) VALUES (?)"),

    get_all: sql!("SELECT id, name FROM users"),
}

#[sqlitex]
struct LogsDb {
    init: sql!(
        "CREATE TABLE IF NOT EXISTS logs (id INTEGER PRIMARY KEY NOT NULL, msg TEXT NOT NULL)"
    ),
    add: sql!("INSERT INTO logs (msg) VALUES (?)"),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;

    // Cloning does not duplicate the connection. All structs point to the same database
    let mut users = UsersDb::new(conn.clone()); // cloning happens here
    let mut logs = LogsDb::new(conn); // last one doesn't need clone

    users.init()?;
    logs.init()?;

    users.add("Alice")?;
    users.add("Bob")?;
    logs.add("started app")?;

    for user in users.get_all()? {
        let user = user?;
        println!("{} {}", user.id, user.name);
    }

    // prints out
    //
    // 1 Alice
    // 2 Bob

    Ok(())
}

