extern crate cc;

fn main() {
    cc::Build::new()
        .file("../../mercurial/mpatch.c")
        .include("../../")
        .compile("mpatch");
}
