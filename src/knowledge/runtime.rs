//! Shim retained for import stability. I/O now lives in `brain-storage`.

pub use brain_storage::{invalidate, load_graph, load_template};
