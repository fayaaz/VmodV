//! The `modv` binary: the native single-process shell. See
//! [`modv_app::native_main`] and `docs/rust-full-rewrite.md`.

use std::process::ExitCode;

fn main() -> ExitCode {
    modv_app::native_main()
}
