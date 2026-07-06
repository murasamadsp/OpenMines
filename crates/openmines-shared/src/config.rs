use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub world_name: String,
    pub port: u16,
    pub world_chunks_w: u32,
    pub world_chunks_h: u32,
    /// Каталог для `SQLite` и слоёв мира (`.mapb`), относительно текущей рабочей директории.
    /// Перекрывается переменной окружения `M3R_DATA_DIR` (абсолютный или относительный путь).
    pub data_dir: String,
    /// См. `LoggingConfig`; секция обязательна в runtime-конфиге.
    pub logging: LoggingConfig,
    /// Настройки фоновых задач.
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
    pub spawn: SpawnConfig,
    pub programmator: ProgrammatorConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpawnConfig {
    pub x: i32,
    pub y: i32,
}

impl Default for SpawnConfig {
    fn default() -> Self {
        Self { x: 10, y: 10 }
    }
}

/// Настройки серверного исполнения программатора.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProgrammatorConfig {
    /// Задержка для прямых действий программы: копание/стройка/геология/хил.
    pub direct_action_delay_us: u64,
    /// Штраф за попытку хода в занятую клетку.
    pub blocked_move_penalty_ms: u64,
    /// Минимальная задержка движения, чтобы программа не крутила busy-loop.
    pub min_move_delay_ms: u64,
}

impl Default for ProgrammatorConfig {
    fn default() -> Self {
        Self {
            direct_action_delay_us: 333_333,
            blocked_move_penalty_ms: 200,
            min_move_delay_ms: 20,
        }
    }
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

/// Настройки вывода логов (см. `crate::logging::init`). Все поля обязательны в
/// runtime-конфиге: отсутствующее значение — ошибка старта, не тихий дефолт.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Директивы `EnvFilter`, если не заданы `RUST_LOG` / `M3R_LOG`.
    pub filter: String,
    pub format: LogFormat,
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
    pub format: LogFormat,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let data =
            fs::read_to_string(path).with_context(|| format!("read server config {path}"))?;
        let cfg: Self =
            serde_json::from_str(&data).with_context(|| format!("parse server config {path}"))?;
        cfg.validate()
            .with_context(|| format!("validate server config {path}"))?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.world_name.trim().is_empty() {
            anyhow::bail!("world_name is empty");
        }
        if self.port == 0 {
            anyhow::bail!("port must be in 1..=65535");
        }
        if self.world_chunks_w == 0 {
            anyhow::bail!("world_chunks_w must be greater than 0");
        }
        if self.world_chunks_h == 0 {
            anyhow::bail!("world_chunks_h must be greater than 0");
        }
        if self.data_dir.trim().is_empty() {
            anyhow::bail!("data_dir is empty");
        }
        self.logging.validate()?;
        self.gameplay
            .validate(self.world_chunks_w, self.world_chunks_h)?;
        Ok(())
    }
}

impl GameplayConfig {
    fn validate(&self, world_chunks_w: u32, world_chunks_h: u32) -> Result<()> {
        self.spawn.validate(world_chunks_w, world_chunks_h)?;
        self.programmator.validate()?;
        Ok(())
    }
}

impl SpawnConfig {
    fn validate(self, world_chunks_w: u32, world_chunks_h: u32) -> Result<()> {
        const SPAWN_MARGIN: i32 = 10;
        let world_cells_w = i32::try_from(world_chunks_w)
            .ok()
            .and_then(|v| v.checked_mul(32))
            .context("world_chunks_w is too large")?;
        let world_cells_h = i32::try_from(world_chunks_h)
            .ok()
            .and_then(|v| v.checked_mul(32))
            .context("world_chunks_h is too large")?;
        if self.x < SPAWN_MARGIN || self.y < SPAWN_MARGIN {
            anyhow::bail!("gameplay.spawn must leave a {SPAWN_MARGIN}-cell margin");
        }
        if self.x + SPAWN_MARGIN >= world_cells_w || self.y + SPAWN_MARGIN >= world_cells_h {
            anyhow::bail!("gameplay.spawn does not fit inside configured world");
        }
        Ok(())
    }
}

impl ProgrammatorConfig {
    fn validate(self) -> Result<()> {
        if self.direct_action_delay_us == 0 {
            anyhow::bail!("gameplay.programmator.direct_action_delay_us must be greater than 0");
        }
        if self.min_move_delay_ms == 0 {
            anyhow::bail!("gameplay.programmator.min_move_delay_ms must be greater than 0");
        }
        Ok(())
    }
}

impl LoggingConfig {
    fn validate(&self) -> Result<()> {
        if self.filter.trim().is_empty() {
            anyhow::bail!("logging.filter is empty");
        }
        if let Some(file) = &self.file {
            file.validate()?;
        }
        Ok(())
    }
}

impl LogFileConfig {
    fn validate(&self) -> Result<()> {
        if self.path.trim().is_empty() {
            anyhow::bail!("logging.file.path is empty");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
        "world_name": "t", "port": 8090, "world_chunks_w": 4, "world_chunks_h": 4,
        "data_dir": ".", "logging": {"filter": "info", "format": "pretty", "file": null},
        "cron": {"hourly_log_enabled": true},
        "gameplay": {"cooldowns": {"dig_ms": 250, "build_ms": 300},
                     "skills": {"upgrade_cost_base": 100},
                     "spawn": {"x": 12, "y": 13},
                     "programmator": {
                       "direct_action_delay_us": 333333,
                       "blocked_move_penalty_ms": 200,
                       "min_move_delay_ms": 20
                     }}
    }"#;

    #[test]
    fn gameplay_values_come_from_config_not_defaults() {
        let c: Config = serde_json::from_str(FULL).unwrap();
        c.validate().unwrap();
        // Значения читаются из JSON, а не из кода (250/300 ≠ дефолтные 200).
        assert_eq!(c.gameplay.cooldowns.dig_ms, 250);
        assert_eq!(c.gameplay.cooldowns.build_ms, 300);
        assert_eq!(c.gameplay.skills.upgrade_cost_base, 100);
        assert_eq!(c.gameplay.spawn.x, 12);
        assert_eq!(c.gameplay.spawn.y, 13);
        assert_eq!(c.gameplay.programmator.direct_action_delay_us, 333_333);
        assert_eq!(c.gameplay.programmator.blocked_move_penalty_ms, 200);
        assert_eq!(c.gameplay.programmator.min_move_delay_ms, 20);
        assert!(c.cron.hourly_log_enabled);
    }

    /// Fail-fast (запрошено): пропущенный геймплей-ключ = ошибка парсинга, а НЕ
    /// тихая подстановка дефолта. Так админ сразу видит, что забыл в `config.json`.
    #[test]
    fn missing_gameplay_key_is_an_error_not_a_silent_default() {
        // Нет секции gameplay целиком.
        let no_gameplay = FULL.replace(
            r#""gameplay": {"cooldowns": {"dig_ms": 250, "build_ms": 300},
                     "skills": {"upgrade_cost_base": 100},
                     "spawn": {"x": 12, "y": 13},
                     "programmator": {
                       "direct_action_delay_us": 333333,
                       "blocked_move_penalty_ms": 200,
                       "min_move_delay_ms": 20
                     }}"#,
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
        let mut no_spawn: serde_json::Value = serde_json::from_str(FULL).unwrap();
        no_spawn["gameplay"]
            .as_object_mut()
            .unwrap()
            .remove("spawn");
        assert!(
            serde_json::from_value::<Config>(no_spawn).is_err(),
            "пропущенный spawn должен быть ошибкой"
        );
        let mut no_programmator: serde_json::Value = serde_json::from_str(FULL).unwrap();
        no_programmator["gameplay"]
            .as_object_mut()
            .unwrap()
            .remove("programmator");
        assert!(
            serde_json::from_value::<Config>(no_programmator).is_err(),
            "пропущенный programmator должен быть ошибкой"
        );
    }

    #[test]
    fn missing_top_level_keys_are_errors_not_silent_defaults() {
        for key in [
            "world_name",
            "port",
            "world_chunks_w",
            "world_chunks_h",
            "data_dir",
            "logging",
            "cron",
        ] {
            let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
            raw.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<Config>(raw).is_err(),
                "missing key must be an error: {key}"
            );
        }
    }

    #[test]
    fn config_load_missing_file_is_error_not_autogenerated() {
        let path = std::env::temp_dir().join(format!(
            "openmines_missing_config_{}_{}.json",
            std::process::id(),
            "strict"
        ));
        let _ = std::fs::remove_file(&path);
        let err = Config::load(path.to_str().unwrap()).unwrap_err();
        assert!(
            err.to_string().contains("read server config"),
            "missing config should fail with read context, got: {err:?}"
        );
        assert!(
            !path.exists(),
            "Config::load must not create missing config"
        );
    }

    #[test]
    fn invalid_infrastructure_values_are_errors() {
        for (key, value) in [
            ("world_name", serde_json::json!("")),
            ("port", serde_json::json!(0)),
            ("world_chunks_w", serde_json::json!(0)),
            ("world_chunks_h", serde_json::json!(0)),
            ("data_dir", serde_json::json!(" ")),
        ] {
            let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
            raw.as_object_mut().unwrap().insert(key.to_string(), value);
            let cfg: Config = serde_json::from_value(raw).unwrap();
            assert!(cfg.validate().is_err(), "invalid {key} must be rejected");
        }
    }

    #[test]
    fn invalid_spawn_values_are_errors() {
        for spawn in [
            serde_json::json!({"x": 9, "y": 13}),
            serde_json::json!({"x": 12, "y": 9}),
            serde_json::json!({"x": 118, "y": 13}),
            serde_json::json!({"x": 12, "y": 118}),
        ] {
            let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
            raw["gameplay"]["spawn"] = spawn;
            let cfg: Config = serde_json::from_value(raw).unwrap();
            assert!(cfg.validate().is_err(), "invalid spawn must be rejected");
        }
    }

    #[test]
    fn invalid_programmator_values_are_errors() {
        for patch in [
            serde_json::json!({"direct_action_delay_us": 0}),
            serde_json::json!({"min_move_delay_ms": 0}),
        ] {
            let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
            let obj = raw["gameplay"]["programmator"].as_object_mut().unwrap();
            for (key, value) in patch.as_object().unwrap() {
                obj.insert(key.clone(), value.clone());
            }
            let cfg: Config = serde_json::from_value(raw).unwrap();
            assert!(
                cfg.validate().is_err(),
                "invalid programmator config must be rejected"
            );
        }
    }

    #[test]
    fn invalid_logging_values_are_errors() {
        let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
        raw["logging"]["filter"] = serde_json::json!("");
        let cfg: Config = serde_json::from_value(raw).unwrap();
        assert!(
            cfg.validate().is_err(),
            "empty logging.filter must be rejected"
        );

        let mut raw: serde_json::Value = serde_json::from_str(FULL).unwrap();
        raw["logging"]["file"] = serde_json::json!({"path": " ", "format": "json"});
        let cfg: Config = serde_json::from_value(raw).unwrap();
        assert!(
            cfg.validate().is_err(),
            "empty logging.file.path must be rejected"
        );
    }
}
