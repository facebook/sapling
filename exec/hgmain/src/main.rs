// Copyright Facebook, Inc. 2018
extern crate hgpython;
use hgpython::HgPython;

fn main() {
    let code = {
        let hgpython = HgPython::new();
        hgpython.run()
    };
    std::process::exit(code);
}
