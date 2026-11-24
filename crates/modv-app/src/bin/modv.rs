//! The `modv` binary: the VmodV single-process shell. See
//! [`modv_app::vmodv_main`].

use std::process::ExitCode;

fn main() -> ExitCode {
    modv_app::vmodv_main()
}
