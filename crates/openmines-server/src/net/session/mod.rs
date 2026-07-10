#![allow(
    clippy::too_many_lines,
    clippy::collapsible_if,
    clippy::map_unwrap_or,
    clippy::redundant_closure_for_method_calls,
    clippy::format_push_string,
    clippy::needless_range_loop,
    clippy::if_not_else,
    clippy::significant_drop_tightening,
    clippy::unnecessary_wraps,
    clippy::manual_let_else,
    clippy::single_match_else,
    clippy::assigning_clones,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::items_after_statements,
    clippy::needless_collect
)]

mod constants;
pub mod outbound;
mod prelude;
pub mod rate_limit;
mod ty_command;
pub mod util;
pub mod wire;

mod auth;
mod connection;
mod handshake;
mod heartbeat;
pub mod hub;
pub mod outbox;
pub mod play;
pub mod player;
pub mod social;
mod state;
pub mod ui;

pub use connection::handle;
