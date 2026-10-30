//! AIPS domain: shared model and the pure (I/O-free) intelligence logic.
//!
//! Everything here is deterministic and unit-tested; services add I/O around it.
pub mod audit;
pub mod flight;
pub mod intel;
pub mod memory;
pub mod model;
pub mod prevent;
pub mod truth;

pub use intel::*;
pub use model::*;
