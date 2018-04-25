// a demo of commandline argument parsing to-be-used in future hg binary
extern crate argparse;

use argparse::argparse::{Arg, Command as ArgParseCommand, ParsedArgs};
use std::collections::HashMap;
use std::env;

trait Command {
    // canonical name of the command, should be consistent with the one in argument parser
    fn name(&self) -> String;
    fn run(&self, &ParsedArgs) -> ();
    fn argparser(&self) -> ArgParseCommand;
}

/// move commit (and descendats)
struct RebaseCommand {}
impl Command for RebaseCommand {
    fn name(&self) -> String {
        format!("rebase")
    }
    fn argparser(&self) -> ArgParseCommand {
        ArgParseCommand::with_name("rebase").arg(Arg::with_name("rev"))
    }
    fn run(&self, args: &ParsedArgs) -> () {
        println!("Running rebase with args {:?}....", args);
    }
}

/// amend the current commit with more changes
struct AmendCommand {}
impl Command for AmendCommand {
    fn name(&self) -> String {
        format!("amend")
    }
    fn argparser(&self) -> ArgParseCommand {
        ArgParseCommand::with_name("amend").arg(Arg::with_name("e"))
    }
    fn run(&self, args: &ParsedArgs) -> () {
        println!("Running amend with args {:?}....", args);
    }
}

fn build_command_table() -> (HashMap<String, Box<Command>>) {
    let commands: Vec<Box<Command>> = vec![Box::new(RebaseCommand {}), Box::new(AmendCommand {})];
    commands.into_iter().map(|c| (c.name(), c)).collect()
}

fn dispatch(args: Vec<String>) -> () {
    let commands = build_command_table();
    let argparser = commands
        .values()
        .fold(ArgParseCommand::with_name("hg"), |a, c| {
            a.subcommand(c.argparser())
        });

    let cmd = argparser.parse(&args);
    match cmd.subcommand {
        Some(ref subcommand) => {
            commands.get(&subcommand.name).map(|c| c.run(&subcommand));
        }
        None => println!("{:?}", cmd),
    }
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    dispatch(args);
}
