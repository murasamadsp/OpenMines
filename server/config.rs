use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_world_name")]
    pub world_name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_chunks_w")]
    pub world_chunks_w: u32,
    #[serde(default = "default_chunks_h")]
    pub world_chunks_h: u32,
    /// Каталог для `SQLite` и слоёв мира (`.mapb`), относительно текущей рабочей директории.
    /// Перекрывается переменной окружения `M3R_DATA_DIR` (абсолютный или относительный путь).
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// См. `LoggingConfig`; при отсутствии ключа в JSON подставляются значения по умолчанию.
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Настройки фоновых задач.
    #[serde(default)]
    pub cron: CronConfig,
    /// Тюнинг геймплея (админ-настраиваемые параметры). ОБЯЗАТЕЛЕН в `config.json`:
    /// нет `#[serde(default)]` → пропущенный ключ = ошибка старта (fail-fast,
    /// «missing field»), а не тихая подстановка. Конфиг — единственный источник
    /// правды в рантайме. Новый параметр = новое поле; растёт организованно.
    pub gameplay: GameplayConfig,
}

/// Корень геймплей-тюнинга. Растёт добавлением секций-суб-структур
/// (`cooldowns`, далее `combat`/`items`/`economy`/…).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameplayConfig {
    pub cooldowns: CooldownConfig,
    pub skills: SkillsConfig,
}

/// Тюнинг скиллов. `upgrade_cost_base` — цена апгрейда в деньгах:
/// `cost = upgrade_cost_base * текущий_уровень` (в C# апгрейд был бесплатным —
/// намеренная экономик-девиация, см. `docs/DEVIATIONS.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    pub upgrade_cost_base: i64,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            upgrade_cost_base: 100,
        }
    }
}

/// Кулдауны действий игрока (мс). Прежние литералы из `play/dig_build.rs`
/// (200ms копание/стройка) — теперь только в `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownConfig {
    pub dig_ms: u64,
    pub build_ms: u64,
}

// `Default` — НЕ для serde-подстановки (поля обязательны при парсинге), а только
// для генерации стартового `config.json` и тест-фикстур. Канонические значения
// живут в одном месте; рантайм всегда читает их из файла.
impl Default for CooldownConfig {
    fn default() -> Self {
        Self {
            dig_ms: 200,
            build_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default = "default_hourly_log_enabled")]
    pub hourly_log_enabled: bool,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            hourly_log_enabled: default_hourly_log_enabled(),
        }
    }
}

const fn default_hourly_log_enabled() -> bool {
    true
}

/// Настройки вывода логов (см. `crate::logging::init`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Директивы `EnvFilter`, если не заданы `RUST_LOG` / `M3R_LOG`.
    #[serde(default = "default_log_filter")]
    pub filter: String,
    #[serde(default)]
    pub format: LogFormat,
    #[serde(default)]
    pub file: Option<LogFileConfig>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            filter: default_log_filter(),
            format: LogFormat::default(),
            file: None,
        }
    }
}

fn default_log_filter() -> String {
    "openmines_server=info,openmines_server::net::session=debug,tokio=warn,h2=warn".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Compact,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    /// Путь вида `logs/server.log` — каталог создаётся, префикс имени для ротации по дням.
    pub path: String,
    #[serde(default = "default_file_log_format")]
    pub format: LogFormat,
}

const fn default_file_log_format() -> LogFormat {
    LogFormat::Json
}

fn default_world_name() -> String {
    "world".into()
}

fn default_data_dir() -> String {
    "data".into()
}

const fn default_port() -> u16 {
    8090
}
/// ~5024×100000 клеток (32×32 на чанк); слои мира ≈3 ГБ.
const fn default_chunks_w() -> u32 {
    157
}
const fn default_chunks_h() -> u32 {
    3125
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        if let Ok(data) = fs::read_to_string(path) {
            Ok(serde_json::from_str(&data)?)
        } else {
            let cfg = Self {
                world_name: default_world_name(),
                port: default_port(),
                world_chunks_w: default_chunks_w(),
                world_chunks_h: default_chunks_h(),
                data_dir: default_data_dir(),
                logging: LoggingConfig::default(),
                cron: CronConfig::default(),
                gameplay: GameplayConfig::default(),
            };
            fs::write(path, serde_json::to_string_pretty(&cfg)?)?;
            Ok(cfg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
        "world_name": "t", "port": 8090, "world_chunks_w": 4, "world_chunks_h": 4,
        "data_dir": ".", "logging": {"filter": "info", "format": "pretty", "file": null},
        "gameplay": {"cooldowns": {"dig_ms": 250, "build_ms": 300},
                     "skills": {"upgrade_cost_base": 100}}
    }"#;

    #[test]
    fn gameplay_values_come_from_config_not_defaults() {
        let c: Config = serde_json::from_str(FULL).unwrap();
        // Значения читаются из JSON, а не из кода (250/300 ≠ дефолтные 200).
        assert_eq!(c.gameplay.cooldowns.dig_ms, 250);
        assert_eq!(c.gameplay.cooldowns.build_ms, 300);
        assert_eq!(c.gameplay.skills.upgrade_cost_base, 100);
    }

    /// Fail-fast (запрошено): пропущенный геймплей-ключ = ошибка парсинга, а НЕ
    /// тихая подстановка дефолта. Так админ сразу видит, что забыл в `config.json`.
    #[test]
    fn missing_gameplay_key_is_an_error_not_a_silent_default() {
        // Нет секции gameplay целиком.
        let no_gameplay = FULL.replace(
            r#""gameplay": {"cooldowns": {"dig_ms": 250, "build_ms": 300},
                     "skills": {"upgrade_cost_base": 100}}"#,
            r#""x": 0"#,
        );
        assert!(
            serde_json::from_str::<Config>(&no_gameplay).is_err(),
            "пропущенный gameplay должен быть ошибкой (fail-fast), а не дефолтом"
        );
        // Нет одного ключа внутри (build_ms) — тоже ошибка.
        let no_build = FULL.replace(r#", "build_ms": 300"#, "");
        assert!(
            serde_json::from_str::<Config>(&no_build).is_err(),
            "пропущенный build_ms должен быть ошибкой"
        );
    }
}
