use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Workload {
    Movement,
    Dig,
    Build,
    BuildCycle,
    Mixed,
}

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub clients: u32,
    pub secs: u64,
    pub move_ms: u64,
    pub synchronized_actions: bool,
    pub workload: Workload,
    pub fixture: bool,
    pub ramp_ms: u64,
    pub drain_secs: u64,
    pub db: String,
    pub player_prefix: String,
}

#[derive(Parser, Debug, Clone)]
#[command(name = "loadtest", about = "Нагрузочный замер OpenMines")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = 8090)]
    port: u16,

    #[arg(long, default_value_t = 500)]
    clients: u32,

    #[arg(long, default_value_t = 30)]
    secs: u64,

    #[arg(long, default_value_t = 200)]
    move_ms: u64,

    /// Start every client action interval on the same tick for burst-stress.
    #[arg(long)]
    synchronized_actions: bool,

    #[arg(long, value_enum, default_value_t = Workload::Movement)]
    workload: Workload,

    /// Require the matching `M3R_LOADTEST_ARENA` server fixture.
    #[arg(long)]
    fixture: bool,

    #[arg(long, default_value_t = 3)]
    ramp_ms: u64,

    #[arg(long, default_value_t = 5)]
    drain_secs: u64,

    #[arg(long, default_value = "data/openmines.db")]
    db: String,

    #[arg(long, default_value = "loadtest")]
    player_prefix: String,
}

impl From<Args> for Config {
    fn from(args: Args) -> Self {
        Self {
            host: args.host,
            port: args.port,
            clients: args.clients,
            secs: args.secs,
            move_ms: args.move_ms,
            synchronized_actions: args.synchronized_actions,
            workload: args.workload,
            fixture: args.fixture,
            ramp_ms: args.ramp_ms,
            drain_secs: args.drain_secs,
            db: args.db,
            player_prefix: args.player_prefix,
        }
    }
}

pub fn parse_args() -> Config {
    Args::parse().into()
}

#[cfg(test)]
pub fn parse_args_from<I, S>(args: I) -> Result<Config, clap::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let mut os_args = vec![std::ffi::OsString::from("loadtest")];
    os_args.extend(args.into_iter().map(Into::into));
    Args::try_parse_from(os_args).map(Into::into)
}
