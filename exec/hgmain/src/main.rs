// Copyright Facebook, Inc. 2018
extern crate hgpython;
use hgpython::{HgEnv, HgPython};

fn main() {
    let code = {
        let e = HgEnv::new();
        let hgpython = HgPython::new(&e);
        hgpython.run()
    };
    std::process::exit(code);
}
