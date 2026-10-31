use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

pub fn run() {
    println!("\n===== ТЕСТ ПРОДУКТИВНОСТІ =====");

    let sqlite = match rusqlite::Connection::open("tea_shop.db") {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Помилка SQLite: {}", error);
            return;
        }
    };

    let client = match redis::Client::open(crate::cache::redis_url()) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("Помилка Redis: {}", error);
            return;
        }
    };

    let mut redis_connection = match client.get_connection() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Помилка Redis: {}", error);
            return;
        }
    };

    let product = match crate::database::get_product_by_id(2) {
        Ok(Some((id, name, category, price, description))) => crate::Product {
            id,
            name,
            category,
            price,
            description,
        },
        Ok(None) => {
            eprintln!("Товар ID=2 не знайдено.");
            return;
        }
        Err(error) => {
            eprintln!("Помилка SQLite: {}", error);
            return;
        }
    };

    let json = match serde_json::to_string(&product) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("Помилка JSON: {}", error);
            return;
        }
    };

    let key = "tea_cache:benchmark:product:2";

    let set_result: redis::RedisResult<()> = redis::cmd("SET")
        .arg(key)
        .arg(&json)
        .query(&mut redis_connection);

    if let Err(error) = set_result {
        eprintln!("Помилка підготовки Redis: {}", error);
        return;
    }

    let statement = sqlite.prepare_cached(
        "SELECT id, name, category, price, description
         FROM products WHERE id = ?1",
    );

    let mut statement = match statement {
        Ok(statement) => statement,
        Err(error) => {
            eprintln!("Помилка SQL: {}", error);
            return;
        }
    };

    for _ in 0..100 {
        let _: rusqlite::Result<(u32, String, String, f64, String)> =
            statement.query_row([2_u32], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            });

        let _: redis::RedisResult<String> = redis::cmd("GET").arg(key).query(&mut redis_connection);
    }

    let mut csv = match std::fs::File::create("benchmark_results.csv") {
        Ok(file) => file,
        Err(error) => {
            eprintln!("Помилка створення CSV: {}", error);
            return;
        }
    };

    if let Err(error) = writeln!(csv, "requests,sqlite_us,redis_us") {
        eprintln!("Помилка запису CSV: {}", error);
        return;
    }

    for count in [100_u32, 1_000, 10_000] {
        println!("\nКількість запитів: {}", count);

        let sqlite_start = Instant::now();

        for _ in 0..count {
            let result: rusqlite::Result<(u32, String, String, f64, String)> =
                statement.query_row([2_u32], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                });

            match result {
                Ok(data) => {
                    black_box(data);
                }
                Err(error) => {
                    eprintln!("Помилка SQLite: {}", error);
                    return;
                }
            }
        }

        let sqlite_time = sqlite_start.elapsed();

        let redis_start = Instant::now();

        for _ in 0..count {
            let result: redis::RedisResult<String> =
                redis::cmd("GET").arg(key).query(&mut redis_connection);

            match result {
                Ok(json) => {
                    let product: crate::Product = match serde_json::from_str(&json) {
                        Ok(product) => product,
                        Err(error) => {
                            eprintln!("Помилка JSON: {}", error);
                            return;
                        }
                    };

                    black_box(product);
                }
                Err(error) => {
                    eprintln!("Помилка Redis: {}", error);
                    return;
                }
            }
        }

        let redis_time = redis_start.elapsed();

        let sqlite_avg = sqlite_time.as_secs_f64() * 1_000_000.0 / count as f64;

        let redis_avg = redis_time.as_secs_f64() * 1_000_000.0 / count as f64;

        println!("SQLite: {:.2} мкс/запит", sqlite_avg);
        println!("Redis:  {:.2} мкс/запит", redis_avg);

        if let Err(error) = writeln!(csv, "{},{:.2},{:.2}", count, sqlite_avg, redis_avg) {
            eprintln!("Помилка запису CSV: {}", error);
            return;
        }

        if redis_avg < sqlite_avg {
            println!("Redis швидший у {:.2} раз(и)", sqlite_avg / redis_avg);
        } else {
            println!("SQLite швидша у {:.2} раз(и)", redis_avg / sqlite_avg);
        }
    }

    let _: redis::RedisResult<i64> = redis::cmd("DEL").arg(key).query(&mut redis_connection);

    println!("\nТестування завершено!");
}
