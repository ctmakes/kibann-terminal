//! The user's files, shared by the app and the CLI: workspace files (the
//! format in [`workspace`], where they're found and whether they may run
//! their commands in [`workspaces`]), and the screens and parts the window
//! is made of ([`screens`]). The daemon doesn't read any of them; it's given
//! what they say.

pub mod screens;
pub mod workspace;
pub mod workspaces;
