use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CACHE_TTL: u32 = 480;

const OLD_KEY: &str = "tea_cache:test:old_connection";
const NEW_KEY: &str = "tea_cache:test:new_connection";

fn clear_key(connection: &mut redis::Connection, key: &str) -> redis::RedisResult<()> {
    let _: i64 = redis::cmd("DEL").arg(key).query(connection)?;

    Ok(())
}

fn measure_request(
    client: &redis::Client,
    cleanup_connection: &mut redis::Connection,
    key: &str,
    json: &str,
    old_method: bool,
) -> Result<f64, Box<dyn Error>> {
    clear_key(cleanup_connection, key)?;

    let start = Instant::now();

    let mut connection = client.get_connection()?;

    let cached: Option<String> = redis::cmd("GET").arg(key).query(&mut connection)?;

    if cached.is_some() {
        return Err("Очікувався CACHE MISS".into());
    }

    if old_method {
        drop(connection);

        let mut second_connection = client.get_connection()?;

        let _: () = redis::cmd("SETEX")
            .arg(key)
            .arg(CACHE_TTL)
            .arg(json)
            .query(&mut second_connection)?;
    } else {
        let _: () = redis::cmd("SETEX")
            .arg(key)
            .arg(CACHE_TTL)
            .arg(json)
            .query(&mut connection)?;
    }

    Ok(start.elapsed().as_secs_f64() * 1_000_000.0)
}

pub fn run() {
    if let Err(error) = run_test() {
        eprintln!("Помилка тестування підключень: {}", error);
    }
}

fn run_test() -> Result<(), Box<dyn Error>> {
    println!("\n===== ТЕСТ ОПТИМІЗАЦІЇ REDIS =====");

    let client = redis::Client::open(crate::cache::redis_url())?;

    let mut cleanup_connection = client.get_connection()?;

    let json = serde_json::json!([
        {
            "product_id": 1,
            "name": "Зелений чай Сенча",
            "units_sold": 120,
            "revenue": 30000.0
        },
        {
            "product_id": 2,
            "name": "Чорний чай Ассам",
            "units_sold": 85,
            "revenue": 17000.0
        },
        {
            "product_id": 3,
            "name": "Ромашковий чай",
            "units_sold": 60,
            "revenue": 10800.0
        }
    ])
    .to_string();

    let mut csv = File::create("connection_benchmark_results.csv")?;

    writeln!(csv, "requests,old_us,new_us")?;

    for count in [100_u32, 500, 1_000] {
        let mut old_total = 0.0;
        let mut new_total = 0.0;

        for i in 0..count {
            if i % 2 == 0 {
                old_total +=
                    measure_request(&client, &mut cleanup_connection, OLD_KEY, &json, true)?;

                new_total +=
                    measure_request(&client, &mut cleanup_connection, NEW_KEY, &json, false)?;
            } else {
                new_total +=
                    measure_request(&client, &mut cleanup_connection, NEW_KEY, &json, false)?;

                old_total +=
                    measure_request(&client, &mut cleanup_connection, OLD_KEY, &json, true)?;
            }
        }

        let old_avg = old_total / count as f64;
        let new_avg = new_total / count as f64;

        println!("\nКількість запитів: {}", count);
        println!("Старий спосіб (2 підключення): {:.2} мкс", old_avg);
        println!("Новий спосіб (1 підключення): {:.2} мкс", new_avg);

        if new_avg < old_avg {
            let improvement = (old_avg - new_avg) / old_avg * 100.0;

            println!("Скорочення часу: {:.2}%", improvement);
        } else {
            println!("Новий спосіб не показав прискорення.");
        }

        writeln!(csv, "{},{:.2},{:.2}", count, old_avg, new_avg)?;
    }

    clear_key(&mut cleanup_connection, OLD_KEY)?;
    clear_key(&mut cleanup_connection, NEW_KEY)?;

    println!("\nРезультати збережено у connection_benchmark_results.csv");
    println!("Тест оптимізації завершено!");

    Ok(())
}
