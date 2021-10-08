/// Snap modules are responsible for setting up control plane
/// RPC services, instantiating engines, loading them into engine
/// groups, and proxying all user setup interactions for those engines.
/// It also services other performance-insensitive functions such as engine
/// creation/destruction, compatibility checks, and policy updates.
pub trait Module {
    fn bootstrap(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}