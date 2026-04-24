//! Re-exports from `brain-domain`. The types live in the pure domain crate so
//! non-UI logic can be unit-tested without pulling in Leptos/axum.
pub use brain_domain::{BrainFilePayload, Edge, EditMode, EditPrefill, Node};
