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
    /// HTTP для map viewer / будущей админки: метаданные мира и сырые чанки (`/api/map/*`).
    /// `None` — не поднимать. Нужен `M3R_MAPVIEWER_TOKEN` (Bearer или заголовок `X-Map-Token`).
    #[serde(default)]
    pub mapviewer_http_port: Option<u16>,
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
                mapviewer_http_port: None,
            };
            fs::write(path, serde_json::to_string_pretty(&cfg)?)?;
            Ok(cfg)
        }
    }
}
