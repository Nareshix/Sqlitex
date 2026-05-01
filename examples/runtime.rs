use sqlitex::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_memory()?;

    // Use execute_runtime for write statements (CREATE, INSERT, UPDATE, DELETE, etc.)
    conn.execute_runtime(
        "CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            price REAL,
            in_stock INTEGER
        )",
    )?;

    // _rows_affected variable is the number of rows modified, which in this case is an insert of 3 rows
    let _rows_affected = conn.execute_runtime(
        "INSERT INTO products (name, price, in_stock) VALUES
        ('Laptop', 999.99, 1),
        ('Mouse', 25.50, 1),
        ('Keyboard', 75.00, 0)",
    )?;

    // Use query_runtime for running SELECT statements
    let results = conn.query_runtime("SELECT * FROM products")?;
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
        .query_runtime("SELECT name, price FROM products WHERE id = 1")?
        .first()?; // or .all()? for all rows

    Ok(())
}


