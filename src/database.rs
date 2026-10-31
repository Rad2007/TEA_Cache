use rusqlite::{Connection, Result};

pub fn initialize_database() -> Result<()> {
    let conn = Connection::open("tea_shop.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS products (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            category TEXT NOT NULL,
            price REAL NOT NULL,
            description TEXT NOT NULL
        )",
        [],
    )?;

    println!("Базу даних SQLite успішно ініціалізовано!");

    Ok(())
}

pub fn insert_product(
    id: u32,
    name: &str,
    category: &str,
    price: f64,
    description: &str,
) -> Result<()> {
    let conn = Connection::open("tea_shop.db")?;

    conn.execute(
        "INSERT OR IGNORE INTO products
         (id, name, category, price, description)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, name, category, price, description],
    )?;

    Ok(())
}

pub fn count_products() -> Result<i64> {
    let conn = Connection::open("tea_shop.db")?;

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))?;

    Ok(count)
}

pub fn get_product_by_id(id: u32) -> Result<Option<(u32, String, String, f64, String)>> {
    let conn = Connection::open("tea_shop.db")?;

    let mut stmt = conn.prepare(
        "SELECT id, name, category, price, description
         FROM products
         WHERE id = ?1",
    )?;

    let mut rows = stmt.query([id])?;

    if let Some(row) = rows.next()? {
        let product = (
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        );

        Ok(Some(product))
    } else {
        Ok(None)
    }
}
