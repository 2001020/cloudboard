//! CloudBoard application logic shared by the Windows and macOS shells.

pub mod app;
pub mod board;
mod chrome;
pub mod input;
pub mod present;
pub mod view;
pub mod widgets;

pub use app::{App, Tool};
pub use input::{Env, Event, Request};
