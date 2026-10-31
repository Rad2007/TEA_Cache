use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::io;
use std::time::Instant;

const CACHE_KEY_PREFIX: &str = "tea_cache:sales_report:rev:v";
const CACHE_TTL: usize = 480;

#[derive(Debug, Serialize, Deserialize)]
struct SalesReportRow {
    product_id: u32,
    name: String,
    units_sold: i64,
    revenue: f64,
}

fn report_cache_key() -> rusqlite::Result<String> {
    let connection = Connection::open("tea_shop.db")?;
    report_cache_key_from_connection(&connection)
}

fn report_cache_key_from_connection(connection: &Connection) -> rusqlite::Result<String> {
    let revision: i64 = connection.query_row(
        "SELECT revision
         FROM cache_revisions
         WHERE cache_name = 'sales_report'",
        [],
        |row| row.get(0),
    )?;

    Ok(format!("{}{}", CACHE_KEY_PREFIX, revision))
}

fn initialize_revisioning(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cache_revisions (
            cache_name TEXT PRIMARY KEY,
            revision INTEGER NOT NULL DEFAULT 0
        );

        INSERT OR IGNORE INTO cache_revisions
            (cache_name, revision)
        VALUES ('sales_report', 0);

        CREATE TRIGGER IF NOT EXISTS trg_sales_insert
        AFTER INSERT ON sales
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;

        CREATE TRIGGER IF NOT EXISTS trg_sales_update
        AFTER UPDATE ON sales
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;

        CREATE TRIGGER IF NOT EXISTS trg_sales_delete
        AFTER DELETE ON sales
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;

        CREATE TRIGGER IF NOT EXISTS trg_products_insert
        AFTER INSERT ON products
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;

        CREATE TRIGGER IF NOT EXISTS trg_products_update
        AFTER UPDATE ON products
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;

        CREATE TRIGGER IF NOT EXISTS trg_products_delete
        AFTER DELETE ON products
        BEGIN
            UPDATE cache_revisions
            SET revision = revision + 1
            WHERE cache_name = 'sales_report';
        END;
        ",
    )?;

    Ok(())
}

pub fn initialize() -> rusqlite::Result<()> {
    let connection = Connection::open("tea_shop.db")?;

    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS sales (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id INTEGER NOT NULL,
            quantity INTEGER NOT NULL CHECK (quantity > 0),
            unit_price REAL NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_sales_product_id
        ON sales(product_id);",
    )?;

    initialize_revisioning(&connection)?;

    Ok(())
}

fn read_number(message: &str) -> Option<u32> {
    println!("{}", message);

    let mut input = String::new();

    match io::stdin().read_line(&mut input) {
        Ok(0) => return None,
        Ok(_) => {}
        Err(error) => {
            eprintln!("Помилка введення: {}", error);
            return None;
        }
    }

    match input.trim().parse::<u32>() {
        Ok(number) if number > 0 => Some(number),
        _ => {
            println!("Введіть ціле число більше нуля!");
            None
        }
    }
}

pub fn record_sale() {
    let product_id = match read_number("Введіть ID товару:") {
        Some(value) => value,
        None => return,
    };

    let quantity = match read_number("Введіть кількість:") {
        Some(value) => value,
        None => return,
    };

    let connection = match Connection::open("tea_shop.db") {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Помилка SQLite: {}", error);
            return;
        }
    };

    let result = connection.execute(
        "INSERT INTO sales (product_id, quantity, unit_price)
         SELECT id, ?2, price
         FROM products
         WHERE id = ?1",
        params![product_id, quantity],
    );

    match result {
        Ok(1) => {
            println!("Продаж успішно збережено в SQLite!");
            println!("Версію кешу змінено. Наступний запит сформує актуальний звіт.");
        }

        Ok(0) => println!("Товар із таким ID не знайдено."),
        Ok(_) => println!("Продаж збережено."),
        Err(error) => eprintln!("Помилка збереження: {}", error),
    }
}

fn calculate_report() -> rusqlite::Result<Vec<SalesReportRow>> {
    let connection = Connection::open("tea_shop.db")?;
    calculate_report_from_connection(&connection)
}

fn calculate_report_from_connection(
    connection: &Connection,
) -> rusqlite::Result<Vec<SalesReportRow>> {
    let mut statement = connection.prepare(
        "SELECT
            p.id,
            p.name,
            COALESCE(SUM(s.quantity), 0),
            COALESCE(SUM(s.quantity * s.unit_price), 0.0)
         FROM products AS p
         LEFT JOIN sales AS s ON s.product_id = p.id
         GROUP BY p.id, p.name
         ORDER BY p.id",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(SalesReportRow {
            product_id: row.get(0)?,
            name: row.get(1)?,
            units_sold: row.get(2)?,
            revenue: row.get(3)?,
        })
    })?;

    rows.collect()
}

fn read_cache(
    connection: &mut redis::Connection,
    key: &str,
) -> Result<Option<Vec<SalesReportRow>>, Box<dyn std::error::Error>> {
    let json: Option<String> = redis::cmd("GET").arg(key).query(connection)?;

    match json {
        Some(json) => Ok(Some(serde_json::from_str(&json)?)),
        None => Ok(None),
    }
}

fn save_cache(
    connection: &mut redis::Connection,
    key: &str,
    rows: &[SalesReportRow],
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(rows)?;

    let _: () = redis::cmd("SETEX")
        .arg(key)
        .arg(CACHE_TTL)
        .arg(json)
        .query(connection)?;

    Ok(())
}

fn print_report(rows: &[SalesReportRow], source: &str, elapsed_us: u128) {
    println!("\n===== ЗВІТ ПРО ПРОДАЖІ =====");
    println!("{}", source);

    for row in rows {
        println!(
            "ID: {} | {} | Продано: {} | Виручка: {:.2} грн",
            row.product_id, row.name, row.units_sold, row.revenue
        );
    }

    let total_units: i64 = rows.iter().map(|row| row.units_sold).sum();
    let total_revenue: f64 = rows.iter().map(|row| row.revenue).sum();

    println!("\nУсього продано: {}", total_units);
    println!("Загальна виручка: {:.2} грн", total_revenue);
    println!("Час отримання звіту: {} мкс", elapsed_us);
}

pub fn show_report() {
    let start = Instant::now();
    let key = match report_cache_key() {
        Ok(key) => key,
        Err(error) => {
            eprintln!("Помилка визначення версії кешу: {}", error);
            return;
        }
    };

    let mut redis_connection = match redis::Client::open(crate::cache::redis_url())
        .and_then(|client| client.get_connection())
    {
        Ok(connection) => Some(connection),

        Err(error) => {
            eprintln!("Redis недоступний: {}", error);
            None
        }
    };

    if let Some(connection) = redis_connection.as_mut() {
        match read_cache(connection, &key) {
            Ok(Some(rows)) => {
                let elapsed = start.elapsed().as_micros();

                print_report(&rows, "CACHE HIT: актуальний звіт із Redis", elapsed);

                return;
            }

            Ok(None) => {
                println!("CACHE MISS: актуальної версії немає.");
            }

            Err(error) => {
                eprintln!("Помилка читання кешу: {}", error);
            }
        }
    }

    match calculate_report() {
        Ok(rows) => {
            if let Some(connection) = redis_connection.as_mut() {
                if let Err(error) = save_cache(connection, &key, &rows) {
                    eprintln!("Помилка збереження кешу: {}", error);
                }
            }

            let elapsed = start.elapsed().as_micros();

            print_report(&rows, "CACHE MISS: звіт сформовано з SQLite", elapsed);
        }

        Err(error) => {
            eprintln!("Помилка формування звіту: {}", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database() -> Connection {
        let connection = Connection::open_in_memory().expect("Не вдалося створити тестову SQLite");

        connection
            .execute_batch(
                "CREATE TABLE products (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL
                );

                CREATE TABLE sales (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    product_id INTEGER NOT NULL,
                    quantity INTEGER NOT NULL,
                    unit_price REAL NOT NULL
                );

                INSERT INTO products (id, name)
                VALUES
                    (1, 'Зелений чай Сенча'),
                    (2, 'Чорний чай Ассам');",
            )
            .expect("Не вдалося підготувати тестові дані");

        initialize_revisioning(&connection).expect("Не вдалося створити систему версій кешу");
        connection
    }

    #[test]
    fn test_report_without_sales() {
        let connection = test_database();

        let report =
            calculate_report_from_connection(&connection).expect("Помилка формування звіту");

        assert_eq!(report.len(), 2);
        assert_eq!(report[0].units_sold, 0);
        assert_eq!(report[0].revenue, 0.0);
        assert_eq!(report[1].units_sold, 0);
        assert_eq!(report[1].revenue, 0.0);
    }

    #[test]
    fn test_sales_calculation() {
        let connection = test_database();

        connection
            .execute_batch(
                "INSERT INTO sales
                    (product_id, quantity, unit_price)
                 VALUES
                    (1, 2, 250.0),
                    (1, 3, 250.0),
                    (2, 1, 200.0);",
            )
            .expect("Не вдалося створити продажі");

        let report =
            calculate_report_from_connection(&connection).expect("Помилка формування звіту");

        assert_eq!(report.len(), 2);

        assert_eq!(report[0].product_id, 1);
        assert_eq!(report[0].units_sold, 5);
        assert_eq!(report[0].revenue, 1250.0);

        assert_eq!(report[1].product_id, 2);
        assert_eq!(report[1].units_sold, 1);
        assert_eq!(report[1].revenue, 200.0);
    }

    #[test]
    fn test_cache_version() {
        let connection = test_database();

        let first_key =
            report_cache_key_from_connection(&connection).expect("Помилка визначення ключа");

        assert_eq!(first_key, "tea_cache:sales_report:rev:v0");

        connection
            .execute(
                "INSERT INTO sales
                 (product_id, quantity, unit_price)
                 VALUES (1, 2, 250.0)",
                [],
            )
            .expect("Не вдалося додати продаж");

        let second_key =
            report_cache_key_from_connection(&connection).expect("Помилка визначення ключа");

        assert_eq!(second_key, "tea_cache:sales_report:rev:v1");

        assert_ne!(first_key, second_key);
    }

    #[test]
    fn test_report_json() {
        let connection = test_database();

        let report =
            calculate_report_from_connection(&connection).expect("Помилка формування звіту");

        let json = serde_json::to_string(&report).expect("Помилка серіалізації JSON");

        let restored: Vec<SalesReportRow> =
            serde_json::from_str(&json).expect("Помилка десеріалізації JSON");

        assert_eq!(restored.len(), report.len());
        assert_eq!(restored[0].product_id, report[0].product_id);
        assert_eq!(restored[0].name, report[0].name);
        assert_eq!(restored[0].units_sold, report[0].units_sold);
        assert_eq!(restored[0].revenue, report[0].revenue);
    }

    #[test]
    fn test_cache_version_after_update_and_delete() {
        let connection = test_database();

        connection
            .execute(
                "INSERT INTO sales
                (product_id, quantity, unit_price)
                VALUES (1, 2, 250.0)",
                [],
            )
            .unwrap();

        let before_update = report_cache_key_from_connection(&connection).unwrap();

        connection
            .execute("UPDATE sales SET quantity = 5 WHERE id = 1", [])
            .unwrap();

        let after_update = report_cache_key_from_connection(&connection).unwrap();

        assert_ne!(before_update, after_update);

        connection
            .execute("DELETE FROM sales WHERE id = 1", [])
            .unwrap();

        let after_delete = report_cache_key_from_connection(&connection).unwrap();

        assert_ne!(after_update, after_delete);
    }

    #[test]
    fn test_cache_version_after_product_update() {
        let connection = test_database();

        let before = report_cache_key_from_connection(&connection).unwrap();

        connection
            .execute(
                "UPDATE products
                SET name = 'Сенча Преміум'
                WHERE id = 1",
                [],
            )
            .unwrap();

        let after = report_cache_key_from_connection(&connection).unwrap();

        assert_ne!(before, after);
    }
}
