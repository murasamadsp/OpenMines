#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::similar_names,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::wildcard_imports,
    clippy::manual_let_else,
    clippy::redundant_pub_crate,
    clippy::too_long_first_doc_paragraph
)]

pub mod config;
pub mod db;
pub mod env_config;
pub mod logging;
pub mod metrics;
pub mod time;
pub mod world;
