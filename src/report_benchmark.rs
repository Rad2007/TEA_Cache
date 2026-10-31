use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

use rusqlite::Connection;

const CACHE_KEY: &str = "tea_cache:benchmark:sales_report";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ReportRow {
    product_id: i64,
    orders_count: i64,
    units_sold: i64,
    revenue: f64,
}

fn calculate_report(connection: &Connection) -> rusqlite::Result<Vec<ReportRow>> {
    let mut statement = connection.prepare(
        "SELECT
            product_id,
            COUNT(*) AS orders_count,
            SUM(quantity) AS units_sold,
            SUM(quantity * price) AS revenue
         FROM sales
         GROUP BY product_id
         ORDER BY product_id",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(ReportRow {
            product_id: row.get(0)?,
            orders_count: row.get(1)?,
            units_sold: row.get(2)?,
            revenue: row.get(3)?,
        })
    })?;

    rows.collect()
}

pub fn run() {
    if let Err(error) = run_test() {
        eprintln!("Помилка тестування звіту: {}", error);
    }
}

fn run_test() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n===== ТЕСТ КЕШУВАННЯ ЗВІТУ =====");

    let mut sqlite = Connection::open_in_memory()?;

    sqlite.execute_batch(
        "CREATE TABLE sales (
            id INTEGER PRIMARY KEY,
            product_id INTEGER NOT NULL,
            quantity INTEGER NOT NULL,
            price REAL NOT NULL
        );",
    )?;

    {
        let transaction = sqlite.transaction()?;

        {
            let mut insert = transaction.prepare(
                "INSERT INTO sales (product_id, quantity, price)
                 VALUES (?1, ?2, ?3)",
            )?;

            for i in 0..20_000_i64 {
                let product_id = i % 3 + 1;
                let quantity = i % 4 + 1;

                let price = match product_id {
                    1 => 250.0,
                    2 => 200.0,
                    _ => 180.0,
                };

                insert.execute(rusqlite::params![product_id, quantity, price])?;
            }
        }

        transaction.commit()?;
    }

    println!("Створено 20 000 тестових продажів.");

    let client = redis::Client::open(crate::cache::redis_url())?;
    let mut redis_connection = client.get_connection()?;

    let report = calculate_report(&sqlite)?;
    let json = serde_json::to_string(&report)?;

    let _: () = redis::cmd("SET")
        .arg(CACHE_KEY)
        .arg(&json)
        .query(&mut redis_connection)?;

    println!("Готовий звіт збережено в Redis.");

    let cached_json: String = redis::cmd("GET")
        .arg(CACHE_KEY)
        .query(&mut redis_connection)?;

    let cached_report: Vec<ReportRow> = serde_json::from_str(&cached_json)?;

    if cached_report.len() != report.len() {
        return Err("Кешований звіт має неправильну кількість рядків".into());
    }

    for _ in 0..10 {
        black_box(calculate_report(&sqlite)?);

        let value: String = redis::cmd("GET")
            .arg(CACHE_KEY)
            .query(&mut redis_connection)?;

        let result: Vec<ReportRow> = serde_json::from_str(&value)?;
        black_box(result);
    }

    let mut csv = std::fs::File::create("report_benchmark_results.csv")?;

    writeln!(csv, "requests,sqlite_us,redis_us")?;

    for count in [100_u32, 500, 1_000] {
        let sqlite_start = Instant::now();

        for _ in 0..count {
            black_box(calculate_report(&sqlite)?);
        }

        let sqlite_elapsed = sqlite_start.elapsed();
        let redis_start = Instant::now();

        for _ in 0..count {
            let value: String = redis::cmd("GET")
                .arg(CACHE_KEY)
                .query(&mut redis_connection)?;

            let result: Vec<ReportRow> = serde_json::from_str(&value)?;

            black_box(result);
        }

        let redis_elapsed = redis_start.elapsed();

        let sqlite_avg = sqlite_elapsed.as_secs_f64() * 1_000_000.0 / count as f64;

        let redis_avg = redis_elapsed.as_secs_f64() * 1_000_000.0 / count as f64;

        println!("\nКількість запитів: {}", count);
        println!("SQLite: {:.2} мкс/запит", sqlite_avg);
        println!("Redis:  {:.2} мкс/запит", redis_avg);

        if redis_avg < sqlite_avg {
            println!("Кеш швидший у {:.2} раз(и)", sqlite_avg / redis_avg);
        } else {
            println!("SQLite швидша у {:.2} раз(и)", redis_avg / sqlite_avg);
        }

        writeln!(csv, "{},{:.2},{:.2}", count, sqlite_avg, redis_avg)?;
    }

    let _: i64 = redis::cmd("DEL")
        .arg(CACHE_KEY)
        .query(&mut redis_connection)?;

    println!("\nРезультати збережено у report_benchmark_results.csv");
    println!("Тестування звіту завершено!");

    Ok(())
}
