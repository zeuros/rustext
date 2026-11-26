//! A Language Server Protocol client implementing rustext's `LanguageServices`
//! (DECISIONS D9): completion, hover and go to definition from any language server.

pub mod client;
pub mod transport;

pub use client::LspServices;
