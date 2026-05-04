use sqlitex::sqlitex;
mod nested_tx;

#[sqlitex]
pub struct ShopDao {
    create_table: sql!(
        " CREATE TABLE Persons (
        PersonID INTEGER NOT NULL,
        LastName TEXT NOT NULL,
        FirstName TEXT NOT NULL,
        Address TEXT,
        Alive INTEGER NOT NULL CHECK (Alive IN (0,1))
    ) STRICT;"
    ),

    insert: sql!(
        "INSERT INTO Persons (PersonID, LastName, FirstName, Address, Alive)
        VALUES (1, 'Smith', 'hi', '123 Main Street', ?);"
    ),

    select: sql!("SELECT * FROM persons"),
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlitex::Connection;

    #[test]
    fn test_shop_flow() -> Result<(), Box<dyn std::error::Error>> {
        // This is the code you had in main()
        let conn = Connection::open_memory().unwrap();
        let mut dao = ShopDao::new(conn);

        dao.create_table()?;
        dao.insert(true)?;

        let results = dao.select()?;

        let first_person = results.first()?.unwrap();

        println!("{:?}", first_person);

        Ok(())
    }
}
