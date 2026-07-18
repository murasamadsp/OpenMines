pub mod helpers;
pub mod parser;
pub mod system;
pub mod types;

#[cfg(test)]
pub mod tests;

pub use system::{next_programmator_deadline, programmator_system};
pub use types::{
    ActionType, LastVariables, PAction, PFunction, ProgrammatorSnapshot, ProgrammatorState,
};
