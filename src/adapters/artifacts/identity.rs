use uuid::Uuid;

/// A fresh execution identity, shared by its output directory and source traces.
pub fn new_run_id() -> String {
    Uuid::new_v4().to_string()
}
