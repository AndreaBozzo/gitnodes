//! Re-exports from `gitnodes-domain`. The types live in the pure domain crate so
//! non-UI logic can be unit-tested without pulling in Leptos/axum.
pub use gitnodes_domain::{BrainFilePayload, Edge, EditMode, EditPrefill, Node};
