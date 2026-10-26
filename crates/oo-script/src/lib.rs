//! Oblivion's scripts: decoding the compiled bytecode the plugins ship, running
//! it, and testing dialogue conditions, which call the same functions.
//!
//! The game's side of every command (what `GetStage` or `Enable` does) lives
//! behind [`Host`], so this crate stays independent of the ECS. The format and
//! the run rules are in `docs/reverse-engineering/script-vm.md`.

pub mod conditions;
pub mod decode;
pub mod vm;

pub use conditions::conditions_pass;
pub use decode::{Arg, DecodeError, Statement, Token, Var, decode};
pub use vm::{Host, Locals, Outcome, Value, run};
