// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// a demo of commandline argument parsing to-be-used in future hg binary
extern crate argparse;

use argparse::argparse::{Command as ArgParseCommand, ParsedArgs};
use argparse::hg_python_commands::add_hg_python_commands;
use std::collections::HashMap;
use std::env;

trait Command {
    // canonical name of the command, should be consistent with the one in argument parser
    fn name(&self) -> String;
    fn run(&self, _: &ParsedArgs);
    fn argparser(&self) -> ArgParseCommand;
}

/// This command is defined solely in the hg rust binary
struct WhereAmICommand {}
impl Command for WhereAmICommand {
    fn name(&self) -> String {
        format!("whereami")
    }
    fn argparser(&self) -> ArgParseCommand {
        ArgParseCommand::with_name("whereami")
    }
    fn run(&self, args: &ParsedArgs) -> () {
        println!("Running whereami with args {:?}....", args);
    }
}

fn build_command_table() -> (HashMap<String, Box<Command>>) {
    let commands: Vec<Box<Command>> = vec![Box::new(WhereAmICommand {})];
    commands.into_iter().map(|c| (c.name(), c)).collect()
}

fn dispatch(args: Vec<String>) -> () {
    let commands = build_command_table();
    let mut argparser = commands
        .values()
        .fold(ArgParseCommand::with_name("hg"), |a, c| {
            a.subcommand(c.argparser())
        });
    // here we're loading other commands definitions so we can parse their arguments
    argparser = add_hg_python_commands(argparser);

    let cmd = argparser.parse(&args);
    match cmd.subcommand {
        Some(ref subcommand) => {
            commands.get(&subcommand.name).map(|c| c.run(&subcommand));
        }
        None => (),
    }
    println!("{:?}", cmd);
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    dispatch(args);
}
