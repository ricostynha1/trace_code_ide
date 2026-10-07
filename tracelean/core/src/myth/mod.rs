//! Myth — every UI surface is a structured document (docs/myth_fable.md).
//!
//! Content parsed into a node tree, queries mark nodes, marked nodes carry
//! style and actions. Rendering, keyboard control, discoverability, and undo
//! are generic services over that model:
//!
//! - `bindings`: capture name → actions map (ui_settings/bindings.json)
//! - `actions`:  static action registry; actions return `Command`s, never
//!               mutate directly — every surface's behavior goes through the
//!               invertible-command/undo-tree machinery
//! - `keymap`:   mode machine over the action registry (ui_settings/keymap.json);
//!               which-key is a query over it, not a feature
//! - `provider`: dynamic action providers — the static capture map is one of
//!               them, beside LSP code actions and traceability actions, which
//!               are computed at the cursor rather than looked up by capture
//! - `surface`:   the `Surface` abstraction + `LineParser` surfaces (file tree)

pub mod actions;
pub mod bindings;
pub mod keymap;
pub mod provider;
pub mod surface;

pub use actions::{ActionCtx, ActionOutcome, ActionRegistry};
pub use keymap::{KeyResult, Keymap, ModeStack};
pub use provider::{
    Action, ActionProvider, CursorCx, LspActionProvider, StaticProvider, TraceActionProvider,
};
pub use surface::{FileTreeSurface, SurfaceNode, SurfaceView};
