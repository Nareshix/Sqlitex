use sqlitex::{Connection, sqlitex};
use std::fs;

#[sqlitex]
struct AppDatabase {
    init: sql!(
        "
        CREATE TABLE IF NOT EXISTS images (
            name TEXT NOT NULL,
            bytes BLOB NOT NULL
        )
    "
    ),
    insert_image: sql!("INSERT INTO images (name, bytes) VALUES (?, ?)"),
    get_image: sql!("SELECT name, bytes FROM images WHERE name = ?"),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("images.db")?;
    let mut db = AppDatabase::new(conn);
    db.init()?;

    // Read the image file from disk into a Vec<u8>
    let image_bytes = fs::read("cat.png")?;

    // Insert into the database by passing a reference to the Vec
    db.insert_image("cat.png", &image_bytes)?;

    // Retrieve the image back from the database
    let results = db.get_image("cat.png")?;
    let doc = results.first()?.unwrap();
    println!(
        "Retrieved document '{}' with {} bytes.",
        doc.name,
        doc.bytes.len()
    );

    // Write it back to the disk to visually compare that it worked
    fs::write("restored_cat.png", &doc.bytes)?;


    // It should result in true
    println!("cat.png == restored_cat.png: {}", fs::read("cat.png")? == fs::read("restored_cat.png")?);

    Ok(())
}

