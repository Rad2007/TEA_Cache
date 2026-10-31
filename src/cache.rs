use redis::{Client, Connection};
use std::error::Error;

use crate::Product;

/// Адреса Redis за замовчуванням (локальний сервер).
pub const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379/";

/// Час життя картки товару в кеші, секунд.
pub const PRODUCT_TTL_SECONDS: u64 = 480;

const PRODUCT_KEY_PREFIX: &str = "tea_cache:product:";

/// Повертає адресу Redis. Її можна змінити змінною середовища REDIS_URL,
/// наприклад, якщо Redis працює в Docker або на іншому комп'ютері.
pub fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string())
}

fn product_key(id: u32) -> String {
    format!("{}{}", PRODUCT_KEY_PREFIX, id)
}

/// Перетворює JSON із кешу на товар. Помилку повертає, а не панікує.
fn decode_product(json: &str) -> Result<Product, serde_json::Error> {
    serde_json::from_str::<Product>(json)
}

/// Кеш товарів на основі Redis. Структура утримує ОДНЕ підключення,
/// яке використовується і для читання, і для запису (оптимізація).
pub struct ProductCache {
    connection: Connection,
}

impl ProductCache {
    /// Створює підключення до Redis і перевіряє його командою PING.
    pub fn connect() -> redis::RedisResult<Self> {
        let client = Client::open(redis_url())?;
        let mut connection = client.get_connection()?;

        let response: String = redis::cmd("PING").query(&mut connection)?;
        println!("Відповідь Redis: {}", response);

        Ok(Self { connection })
    }

    /// Читає товар із кешу. Ok(None) означає CACHE MISS.
    /// Пошкоджений запис видаляється і також вважається CACHE MISS.
    pub fn get_product(&mut self, id: u32) -> Result<Option<Product>, Box<dyn Error>> {
        let key = product_key(id);

        let json: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query(&mut self.connection)?;

        match json {
            None => Ok(None),

            Some(data) => match decode_product(&data) {
                Ok(product) => Ok(Some(product)),

                Err(error) => {
                    eprintln!("Пошкоджені дані в кеші ({}): запис видаляється", error);

                    let _: i64 = redis::cmd("DEL")
                        .arg(&key)
                        .query(&mut self.connection)?;

                    Ok(None)
                }
            },
        }
    }

    /// Зберігає товар у кеші з обмеженим часом життя.
    pub fn save_product(&mut self, product: &Product) -> Result<(), Box<dyn Error>> {
        let key = product_key(product.id);
        let json = serde_json::to_string(product)?;

        let _: () = redis::cmd("SETEX")
            .arg(&key)
            .arg(PRODUCT_TTL_SECONDS)
            .arg(&json)
            .query(&mut self.connection)?;

        println!("Товар збережено в Redis на {} секунд!", PRODUCT_TTL_SECONDS);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Product {
        Product {
            id: 7,
            name: "Тестовий чай".to_string(),
            category: "Тест".to_string(),
            price: 99.5,
            description: "Опис".to_string(),
        }
    }

    #[test]
    fn test_product_key() {
        assert_eq!(product_key(2), "tea_cache:product:2");
    }

    #[test]
    fn test_decode_valid_json() {
        let json = serde_json::to_string(&sample()).unwrap();
        let product = decode_product(&json).unwrap();

        assert_eq!(product.id, 7);
        assert_eq!(product.name, "Тестовий чай");
        assert_eq!(product.price, 99.5);
    }

    #[test]
    fn test_decode_broken_json_returns_error() {
        assert!(decode_product("{це не json").is_err());
        assert!(decode_product("").is_err());
        assert!(decode_product("{\"id\": 1}").is_err());
    }
}
