//! Execution engines — orchestrate multi-step workflows for GBA commands.
//!
//! Each engine corresponds to a top-level CLI command (`init`, `plan`, `run`)
//! and encapsulates the full workflow including prompt rendering, agent calls,
//! and file I/O.

pub mod init;
pub mod plan;
