//! The `modv` binary: the VmodV single-process shell. See
//! [`modv_app::vmodv_main`] and `docs/rust-full-rewrite.md`.

use std::process::ExitCode;

fn main() -> ExitCode {
    modv_app::vmodv_main()
}
