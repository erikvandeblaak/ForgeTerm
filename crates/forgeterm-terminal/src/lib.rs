//! ForgeTerm terminal engine.
//!
//! UI-agnostic: spawns a real PTY (ConPTY on Windows, a Unix PTY on Linux), runs the
//! VT parser (`alacritty_terminal`) on a background I/O thread and exposes
//! resolved, renderer-friendly [`Snapshot`]s plus input encoders. No GUI types leak
//! out of this crate, so it can be tested headless.

mod input;
mod palette;
mod quote;
mod session;
mod shell;
mod snapshot;

pub use input::{
    Key, Modifiers, MouseButton, encode_key, encode_mouse, encode_paste, paste_needs_confirmation,
};
pub use palette::{Palette, Rgb};
pub use quote::{quote_path, quote_paths};
pub use session::{GridSize, Modes, Session, SessionConfig, SessionEvent};
pub use shell::{ShellKind, ShellSpec, default_shell, detect_shells};
pub use snapshot::{CellFlags, CellView, CursorShape, CursorView, Snapshot};
