//! Regenerate the GitHub Pages module catalogue from the app's registry.
//!
//! Run with `make module-docs` (Docker), or directly:
//!   cargo run -p modv-app --bin gen-module-docs -- [--isf-dir DIR] [--out FILE]
//!
//! The output is checked into `docs/modules/modules.json`; Pages serves the
//! committed file, so no Rust toolchain is needed to publish the site.

use std::path::{Path, PathBuf};

use modv_app::module_docs::write_documents;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut isf_dir = root.join("resources/isf");
    let mut output = root.join("docs/modules");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isf-dir" => {
                isf_dir = PathBuf::from(args.next().expect("--isf-dir needs a path"));
            }
            "--out" => {
                output = PathBuf::from(args.next().expect("--out needs a directory"));
            }
            other => {
                eprintln!("gen-module-docs: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    if !isf_dir.is_dir() {
        eprintln!(
            "gen-module-docs: ISF corpus missing at {}",
            isf_dir.display()
        );
        std::process::exit(1);
    }
    match write_documents(&isf_dir, &output) {
        Ok(()) => println!("gen-module-docs: wrote {}", output.display()),
        Err(error) => {
            eprintln!("gen-module-docs: {error}");
            std::process::exit(1);
        }
    }
}
