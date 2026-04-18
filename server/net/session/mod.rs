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
mod dispatch;
mod outbound;
mod prelude;
pub mod util;
pub mod wire;

mod auth;
mod connection;
pub mod play;
mod player;
mod social;
mod ui;

pub use connection::handle;
