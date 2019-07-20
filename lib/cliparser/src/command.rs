// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub struct Command {
    name: String,
    aliases: Vec<String>,
    subcommands: Vec<Command>, // TODO - some way to define the flags this supports
}

impl Command {
    pub fn new() -> Self {
        unimplemented!()
    }

    pub fn has_alias(&self, alias: &str) -> bool {
        unimplemented!()
    }

    pub fn has_subcommand(&self, subcommand_name: &str) -> bool {
        unimplemented!()
    }

    pub fn name(&self) -> &str {
        unimplemented!()
    }

    pub fn builder() -> CommandBuilder {
        unimplemented!()
    }
}

pub struct CommandBuilder {
    name: String,
    aliases: Vec<String>,
    subcommands: Vec<Command>,
}

// example api usage:
// Command::builder()
//     .with_name("cloud")
//     .with_aliases(vec!["commitcloud", "cc"])
//     .with_subcommand(
//         Command::builder()
//             .with_name("authenticate")
//             .with_alias("login")
//             .build()
//     )
//     .with_subcommand(
//         Command::builder()
//             .with_name("rejoin")
//             .with_alias("rj")
//             .build()
//     )
//     .build();
impl CommandBuilder {
    fn new() -> Self {
        unimplemented!()
    }

    pub fn with_alias(&mut self, alias: &str) -> Self {
        unimplemented!()
    }

    pub fn with_aliases(&mut self, aliases: Vec<&str>) -> Self {
        unimplemented!()
    }

    pub fn with_subcommand(&mut self, subcommand: Command) -> Self {
        unimplemented!()
    }

    pub fn with_subcommands(&mut self, subcommands: Vec<Command>) -> Self {
        unimplemented!()
    }

    pub fn new_subcommand(&mut self, builder: CommandBuilder) -> Self {
        unimplemented!()
    }

    pub fn build(self) -> Command {
        unimplemented!()
    }
}
