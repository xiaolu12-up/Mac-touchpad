pub mod config;
pub mod hid;
pub mod contacts;
pub mod gesture;
pub mod input;
pub mod overlay;
pub mod speed;
pub mod startup;
pub mod window;

// Re-exports for convenience
pub use config::Config;
pub use gesture::engine::GestureEngine;
pub use window::start_message_loop;
