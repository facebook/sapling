// Copyright Facebook, Inc. 2018
#[cfg(feature = "with_chg")]
extern crate dirs;
extern crate encoding;
extern crate hgpython;
#[cfg(feature = "with_chg")]
extern crate libc;
use hgpython::HgPython;

#[cfg(feature = "with_chg")]
mod chg;
#[cfg(feature = "with_chg")]
use chg::maybe_call_chg;

/// Execute a command, using an embedded interpreter
/// This function does not return
fn call_embedded_python() {
    let code = {
        let hgpython = HgPython::new();
        hgpython.run()
    };
    std::process::exit(code);
}

fn main() {
    #[cfg(feature = "with_chg")]
    maybe_call_chg();
    call_embedded_python();
}
