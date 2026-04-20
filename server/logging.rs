//! Инициализация `tracing`: фильтр, формат, опциональный файл с ротацией по дням.

use crate::config::{LogFileConfig, LogFormat, LoggingConfig};
use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};
use std::path::Path;
use tracing_subscriber::fmt::time::SystemTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry, fmt};

/// Держит поток записи в файл; не бросать до выхода процесса.
pub struct LoggingGuard {
    _file_worker: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// `RUST_LOG` или `M3R_LOG` (алиас), иначе строка из конфига.
fn build_env_filter(cfg: &LoggingConfig) -> Result<EnvFilter> {
    std::env::var("RUST_LOG")
        .or_else(|_| std::env::var("M3R_LOG"))
        .ok()
        .map_or_else(
            || EnvFilter::try_new(&cfg.filter).context("invalid logging.filter in config"),
            |s| EnvFilter::try_new(&s).context("invalid RUST_LOG / M3R_LOG"),
        )
}

fn daily_rolling_appender(
    file_cfg: &LogFileConfig,
) -> Result<tracing_appender::rolling::RollingFileAppender> {
    let path = Path::new(&file_cfg.path);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("server");
    std::fs::create_dir_all(dir).with_context(|| format!("create log dir {}", dir.display()))?;
    Ok(tracing_appender::rolling::daily(dir, stem))
}

fn log_panic_to_stderr(info: &std::panic::PanicHookInfo<'_>) {
    let loc = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "unknown location".to_string());
    let payload = info.payload();
    let msg = payload
        .downcast_ref::<&'static str>()
        .copied()
        .map(String::from)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| format!("{payload:?}"));
    let _ = writeln!(
        std::io::stderr(),
        "[openmines-server PANIC] {loc}\n  message: {msg}"
    );
    if std::env::var("RUST_BACKTRACE")
        .ok()
        .is_some_and(|v| !matches!(v.trim(), "" | "0"))
    {
        let bt = std::backtrace::Backtrace::capture();
        let _ = writeln!(std::io::stderr(), "{bt}");
    }
    let _ = std::io::stderr().flush();
}

/// Вызывать в самом начале `main` до `logging::init`, чтобы паники при старте не «терялись».
pub fn install_early_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_panic_to_stderr(info);
        previous(info);
    }));
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // stderr уже пишет цепочка из `install_early_panic_hook`; здесь — в subscriber.
        tracing::error!(target: "openmines_server::panic", %info, "panic (see stderr for message + RUST_BACKTRACE)");
        previous(info);
        if std::env::var("M3R_ABORT_ON_PANIC")
            .ok()
            .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        {
            let _ = writeln!(
                std::io::stderr(),
                "[openmines-server] M3R_ABORT_ON_PANIC: exiting with status 101"
            );
            let _ = std::io::stderr().flush();
            std::process::exit(101);
        }
    }));
}

fn try_init_registry<S>(subscriber: S) -> Result<()>
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| anyhow::anyhow!("tracing already initialized"))
}

fn init_stderr_only(filter: EnvFilter, console: LogFormat, timer: SystemTime) -> Result<()> {
    let ansi = std::io::stderr().is_terminal();
    match console {
        LogFormat::Pretty => try_init_registry(
            Registry::default().with(filter).with(
                fmt::layer()
                    .pretty()
                    .with_writer(std::io::stderr)
                    .with_ansi(ansi)
                    .with_timer(timer),
            ),
        ),
        LogFormat::Compact => try_init_registry(
            Registry::default().with(filter).with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .with_timer(timer),
            ),
        ),
        LogFormat::Json => try_init_registry(
            Registry::default().with(filter).with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .with_timer(timer),
            ),
        ),
    }
}

#[allow(clippy::too_many_lines)]
fn init_stderr_and_file(
    filter: EnvFilter,
    console: LogFormat,
    file_fmt: LogFormat,
    nb: NonBlockingWriter,
    timer: SystemTime,
) -> Result<()> {
    let console_ansi = std::io::stderr().is_terminal();
    match (console, file_fmt) {
        (LogFormat::Pretty, LogFormat::Pretty) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(std::io::stderr)
                        .with_ansi(console_ansi)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Pretty, LogFormat::Compact) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(std::io::stderr)
                        .with_ansi(console_ansi)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Pretty, LogFormat::Json) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(std::io::stderr)
                        .with_ansi(console_ansi)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Compact, LogFormat::Pretty) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Compact, LogFormat::Compact) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Compact, LogFormat::Json) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Json, LogFormat::Pretty) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .pretty()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Json, LogFormat::Compact) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
        (LogFormat::Json, LogFormat::Json) => try_init_registry(
            Registry::default()
                .with(filter)
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_ansi(false)
                        .with_timer(timer),
                )
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(nb)
                        .with_ansi(false)
                        .with_timer(timer),
                ),
        ),
    }
}

type NonBlockingWriter = tracing_appender::non_blocking::NonBlocking;

/// Инициализирует глобальный subscriber. Вызывать один раз после загрузки конфига.
pub fn init(cfg: &LoggingConfig) -> Result<LoggingGuard> {
    let filter = build_env_filter(cfg)?;
    let timer = SystemTime;
    let mut file_worker = None;

    if let Some(ref fc) = cfg.file {
        let appender = daily_rolling_appender(fc)?;
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);
        file_worker = Some(guard);
        init_stderr_and_file(filter, cfg.format, fc.format, non_blocking, timer)?;
    } else {
        init_stderr_only(filter, cfg.format, timer)?;
    }

    install_panic_hook();

    Ok(LoggingGuard {
        _file_worker: file_worker,
    })
}
