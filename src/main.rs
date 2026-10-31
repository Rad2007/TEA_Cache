mod benchmark;
mod cache;
mod connection_benchmark;
mod database;
mod report_benchmark;
mod sales;

use std::io;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Product {
    id: u32,
    name: String,
    category: String,
    price: f64,
    description: String,
}

#[derive(Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
    total_time_us: u128,
    requests: u64,
}

impl CacheStats {
    fn show(&self) {
        println!("\n===== СТАТИСТИКА КЕШУ =====");
        println!("CACHE HIT: {}", self.hits);
        println!("CACHE MISS: {}", self.misses);
        println!("Усього запитів: {}", self.requests);

        if self.requests > 0 {
            let hit_rate = self.hits as f64 / self.requests as f64 * 100.0;
            let average = self.total_time_us as f64 / self.requests as f64;

            println!("Частка CACHE HIT: {:.2}%", hit_rate);
            println!("Середній час запиту: {:.2} мкс", average);
        }
    }
}

fn search_product(id: u32, stats: &mut CacheStats, cache: &mut Option<cache::ProductCache>) {
    let start = Instant::now();

    // Якщо підключення немає, робимо спробу відновити його.
    if cache.is_none() {
        match cache::ProductCache::connect() {
            Ok(connection) => *cache = Some(connection),
            Err(error) => eprintln!("Redis недоступний: {}", error),
        }
    }

    let cached = cache.as_mut().map(|redis_cache| redis_cache.get_product(id));

    let product = match cached {
        Some(Ok(Some(product))) => {
            stats.hits += 1;
            println!("\nCACHE HIT — товар отримано з Redis");
            Some(product)
        }

        Some(Ok(None)) => {
            stats.misses += 1;
            println!("\nCACHE MISS — пошук у SQLite");

            load_from_database(id, cache.as_mut())
        }

        Some(Err(error)) => {
            println!("Помилка Redis: {}", error);
            println!("Переходимо до SQLite");

            // Підключення могло зламатися: скидаємо його до наступного запиту.
            *cache = None;

            load_from_database(id, None)
        }

        None => {
            println!("Переходимо до SQLite");

            load_from_database(id, None)
        }
    };

    let elapsed = start.elapsed().as_micros();

    stats.requests += 1;
    stats.total_time_us += elapsed;

    match product {
        Some(product) => {
            println!("ID: {}", product.id);
            println!("Назва: {}", product.name);
            println!("Категорія: {}", product.category);
            println!("Ціна: {} грн", product.price);
            println!("Опис: {}", product.description);
        }

        None => {
            println!("Товар не знайдено.");
        }
    }

    println!("Час запиту: {} мкс", elapsed);
}

fn load_from_database(id: u32, cache: Option<&mut cache::ProductCache>) -> Option<Product> {
    match database::get_product_by_id(id) {
        Ok(Some(data)) => {
            let product = Product {
                id: data.0,
                name: data.1,
                category: data.2,
                price: data.3,
                description: data.4,
            };

            if let Some(redis_cache) = cache {
                if let Err(error) = redis_cache.save_product(&product) {
                    eprintln!("Не вдалося оновити кеш: {}", error);
                }
            }

            Some(product)
        }

        Ok(None) => None,

        Err(error) => {
            eprintln!("Помилка SQLite: {}", error);
            None
        }
    }
}

fn main() {
    if let Err(error) = database::initialize_database() {
        eprintln!("Помилка SQLite: {}", error);
        return;
    }

    if let Err(error) = sales::initialize() {
        eprintln!("Помилка ініціалізації продажів: {}", error);
        return;
    }

    let mut product_cache = match cache::ProductCache::connect() {
        Ok(connection) => Some(connection),
        Err(error) => {
            eprintln!("Увага: Redis недоступний: {}", error);
            None
        }
    };

    let products = vec![
        Product {
            id: 1,
            name: "Зелений чай Сенча".to_string(),
            category: "Зелений чай".to_string(),
            price: 250.0,
            description: "Японський зелений чай із насиченим свіжим смаком та ароматом."
                .to_string(),
        },
        Product {
            id: 2,
            name: "Чорний чай Ассам".to_string(),
            category: "Чорний чай".to_string(),
            price: 200.0,
            description: "Індійський чорний чай з насиченим терпким смаком та ароматом."
                .to_string(),
        },
        Product {
            id: 3,
            name: "Ромашковий чай".to_string(),
            category: "Трав'яний чай".to_string(),
            price: 180.0,
            description:
                "Чай із сушених квітів ромашки з ніжним ароматом та заспокійливим ефектом."
                    .to_string(),
        },
    ];

    for product in &products {
        if let Err(error) = database::insert_product(
            product.id,
            &product.name,
            &product.category,
            product.price,
            &product.description,
        ) {
            eprintln!("Помилка збереження товару: {}", error);
            return;
        }
    }
    println!("Товари успішно збережено в SQLite!");

    match database::count_products() {
        Ok(count) => println!("Товарів у базі SQLite: {}", count),
        Err(error) => eprintln!("Помилка підрахунку товарів: {}", error),
    }

    let mut stats = CacheStats::default();

    loop {
        println!("\n========== ЧАЙНИЙ МАГАЗИН ==========");
        println!("1. Переглянути каталог");
        println!("2. Знайти товар за ID");
        println!("3. Статистика кешу");
        println!("4. Тест продуктивності");
        println!("5. Тест кешування звіту");
        println!("6. Зареєструвати продаж");
        println!("7. Звіт про продажі");
        println!("8. Тест оптимізації Redis");
        println!("0. Вийти");
        println!("Оберіть дію:");

        let mut input = String::new();

        if io::stdin().read_line(&mut input).is_err() {
            println!("Помилка введення.");
            continue;
        }

        match input.trim() {
            "1" => {
                println!("\n===== КАТАЛОГ ЧАЮ =====");

                for product in &products {
                    println!(
                        "ID: {} | {} | {} грн",
                        product.id, product.name, product.price
                    );
                }
            }

            "2" => {
                println!("Введіть ID товару:");

                let mut id_input = String::new();

                if io::stdin().read_line(&mut id_input).is_err() {
                    println!("Помилка введення.");
                    continue;
                }

                match id_input.trim().parse::<u32>() {
                    Ok(id) => search_product(id, &mut stats, &mut product_cache),
                    Err(_) => println!("Введіть коректний ID!"),
                }
            }

            "3" => stats.show(),

            "4" => benchmark::run(),

            "5" => report_benchmark::run(),

            "6" => sales::record_sale(),

            "7" => sales::show_report(),

            "8" => connection_benchmark::run(),

            "0" => {
                println!("До нових чаювань!");
                break;
            }

            _ => println!("Невідома команда."),
        }
    }
}
