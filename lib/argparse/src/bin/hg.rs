// a demo of commandline argument parsing to-be-used in future hg binary
extern crate argparse;

use argparse::argparse::{Arg, Command};
use std::env;

fn hg_parser() -> Command {
    Command::with_name("hg")
        .arg(Arg::with_name("repository").short(b'R').requires_value())
        .arg(Arg::with_name("cwd").requires_value())
        .arg(Arg::with_name("config").requires_value())
        .subcommand(Command::with_name("rebase"))
        .subcommand(Command::with_name("update").alias("checkout").alias("co"))
        .subcommand(Command::with_name("commit").alias("ci"))
        .subcommand(Command::with_name("amend"))
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let parser = hg_parser();
    let cmd = parser.parse(&args);
    println!("{:?}", cmd)
}
