//! `vena-forge` — book → `.vena` package. The same Rust crate the app embeds for
//! on-device forging and the maintainer CLI uses for flagship prebuilt packages.

pub mod forge;
pub mod import;
pub mod ledger;

pub use forge::{forge_to_db, ForgeStats};
pub use import::{import_path, ImportedBook};
pub use ledger::{extract_with_model, load_curated, Ledger};
