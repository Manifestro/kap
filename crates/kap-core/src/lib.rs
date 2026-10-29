//! KAP core: isolation, history, and decisions for work done by AI agents.
//!
//! The core is deliberately small. It knows how to create the `.kap/` store, how to give an
//! attempt its own branch and worktree, and how to record what happened. It never talks to a
//! model and never drives an agent's loop — adapters do that, on top of this API.

pub mod attempt;
pub mod db;
pub mod decision;
pub mod error;
pub mod events;
pub mod git;
pub mod model;
pub mod objects;
pub mod repo;

pub use error::{Error, Result};
