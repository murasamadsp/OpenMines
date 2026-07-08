use clap::Parser;

fn parse_rich_bool(s: &str) -> Result<bool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        value => Err(format!(
            "invalid boolean value {value:?}; expected one of: 1,true,yes,on,0,false,no,off"
        )),
    }
}

#[derive(Parser, Debug, Clone)]
#[command(name = "openmines-server", about = "OpenMines — игровой сервер (Rust)")]
pub struct Args {
    /// Validate config/resources/state directory and exit without starting TCP/admin servers
    #[arg(long)]
    pub doctor: bool,

    /// Force regeneration of the world map on startup
    #[arg(
        long,
        aliases = ["regen-world"],
        env = "M3R_REGEN_WORLD",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = parse_rich_bool,
        default_value = "false"
    )]
    pub regen: bool,

    /// Port override for TCP server
    #[arg(long, env = "M3R_PORT")]
    pub port: Option<u16>,

    /// Path to server configuration file
    #[arg(long, default_value = "configs/config.json")]
    pub config: String,

    /// Path to cell definitions file
    #[arg(long, default_value = "configs/cells.json")]
    pub cells_config: String,

    /// Path to buildings configuration file
    #[arg(long, default_value = "configs/buildings.json")]
    pub buildings_config: String,

    /// Set to true/false to enable/disable Ctrl+C/SIGINT handling
    #[arg(
        long,
        env = "M3R_USE_CTRL_C",
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = parse_rich_bool,
        default_value = "true"
    )]
    pub use_ctrl_c: bool,

    /// Promotes listed player names to role = Admin on startup (comma-separated list)
    #[arg(long, env = "M3R_GRANT_ADMIN")]
    pub grant_admin: Option<String>,

    /// Override the state directory (e.g. `data_dir` in config)
    #[arg(long, env = "M3R_DATA_DIR")]
    pub data_dir: Option<String>,

    /// Port for Admin HTTP server
    #[arg(long, env = "M3R_ADMIN_PORT", default_value = "8091")]
    pub admin_port: u16,

    /// Secret authorization token for Admin Panel
    #[arg(long, env = "M3R_ADMIN_TOKEN")]
    pub admin_token: Option<String>,
}

impl Args {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Глобальный мьютекс для сериализации тестов, изменяющих переменные окружения
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn parse_from_env_and_args<I, S>(args: I, envs: &[(&str, &str)]) -> Result<Args, clap::Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString> + Clone,
    {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Полностью очищаем все переменные окружения, которые использует парсер,
        // чтобы избежать утечки состояния между тестами
        let env_vars_to_clear = [
            "M3R_REGEN_WORLD",
            "M3R_PORT",
            "M3R_USE_CTRL_C",
            "M3R_GRANT_ADMIN",
            "M3R_DATA_DIR",
            "M3R_ADMIN_PORT",
            "M3R_ADMIN_TOKEN",
        ];
        unsafe {
            for var in &env_vars_to_clear {
                std::env::remove_var(var);
            }
            for &(k, v) in envs {
                std::env::set_var(k, v);
            }
        }

        let mut full_args = vec![std::ffi::OsString::from("openmines-server")];
        full_args.extend(args.into_iter().map(Into::into));

        let res = Args::try_parse_from(full_args);

        // Очищаем переменные после теста
        unsafe {
            for var in &env_vars_to_clear {
                std::env::remove_var(var);
            }
        }

        res
    }

    #[test]
    fn test_parse_regen_flag_no_args() {
        let args = parse_from_env_and_args([] as [&str; 0], &[]).unwrap();
        assert!(!args.regen);
    }

    #[test]
    fn test_admin_token_has_no_implicit_default() {
        let args = parse_from_env_and_args([] as [&str; 0], &[]).unwrap();
        assert_eq!(args.admin_token, None);

        let args =
            parse_from_env_and_args([] as [&str; 0], &[("M3R_ADMIN_TOKEN", "secret")]).unwrap();
        assert_eq!(args.admin_token.as_deref(), Some("secret"));
    }

    #[test]
    fn test_parse_doctor_flag() {
        let args = parse_from_env_and_args(["--doctor"], &[]).unwrap();
        assert!(args.doctor);
        assert!(!args.regen);
    }

    #[test]
    fn test_parse_regen_flag_cli_flag() {
        let args = parse_from_env_and_args(["--regen"], &[]).unwrap();
        assert!(args.regen);

        let args2 = parse_from_env_and_args(["--regen-world"], &[]).unwrap();
        assert!(args2.regen);
    }

    #[test]
    fn test_parse_regen_flag_env_true() {
        let args =
            parse_from_env_and_args([] as [&str; 0], &[("M3R_REGEN_WORLD", "true")]).unwrap();
        assert!(args.regen);

        let args2 = parse_from_env_and_args([] as [&str; 0], &[("M3R_REGEN_WORLD", "1")]).unwrap();
        assert!(args2.regen);
    }

    #[test]
    fn test_parse_regen_flag_env_false() {
        let args =
            parse_from_env_and_args([] as [&str; 0], &[("M3R_REGEN_WORLD", "false")]).unwrap();
        assert!(!args.regen);

        let args2 = parse_from_env_and_args([] as [&str; 0], &[("M3R_REGEN_WORLD", "0")]).unwrap();
        assert!(!args2.regen);
    }

    #[test]
    fn test_parse_regen_flag_cli_overrides_env() {
        let args = parse_from_env_and_args(["--regen"], &[("M3R_REGEN_WORLD", "false")]).unwrap();
        assert!(args.regen);
    }

    #[test]
    fn test_parse_port_override_absent() {
        let args = parse_from_env_and_args([] as [&str; 0], &[]).unwrap();
        assert_eq!(args.port, None);
    }

    #[test]
    fn test_parse_port_override_valid() {
        let args = parse_from_env_and_args([] as [&str; 0], &[("M3R_PORT", "19090")]).unwrap();
        assert_eq!(args.port, Some(19090));

        let args2 = parse_from_env_and_args(["--port", "9090"], &[]).unwrap();
        assert_eq!(args2.port, Some(9090));
    }

    #[test]
    fn test_parse_port_override_invalid() {
        assert!(parse_from_env_and_args([] as [&str; 0], &[("M3R_PORT", "abc")]).is_err());
        assert!(parse_from_env_and_args(["--port", "abc"], &[]).is_err());
    }

    #[test]
    fn test_use_ctrl_c_default() {
        let args = parse_from_env_and_args([] as [&str; 0], &[]).unwrap();
        assert!(args.use_ctrl_c);
    }

    #[test]
    fn test_use_ctrl_c_env_false() {
        let args =
            parse_from_env_and_args([] as [&str; 0], &[("M3R_USE_CTRL_C", "false")]).unwrap();
        assert!(!args.use_ctrl_c);
    }
}
