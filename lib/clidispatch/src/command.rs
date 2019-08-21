// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::{errors::DispatchError, io::IO, repo::Repo};
use cliparser::parser::{Flag, ParseOutput, StructFlags};
use std::{collections::BTreeMap, ops::Deref};

pub enum CommandFunc {
    NoRepo(Box<dyn Fn(ParseOutput, &mut IO) -> Result<u8, DispatchError>>),
    InferRepo(Box<dyn Fn(ParseOutput, &mut IO, Option<Repo>) -> Result<u8, DispatchError>>),
    Repo(Box<dyn Fn(ParseOutput, &mut IO, Repo) -> Result<u8, DispatchError>>),
}

pub struct CommandDefinition {
    name: String,
    doc: String,
    flags_func: fn() -> Vec<Flag>,
    func: CommandFunc,
}

impl CommandDefinition {
    pub fn new(
        name: impl ToString,
        doc: impl ToString,
        flags_func: fn() -> Vec<Flag>,
        func: CommandFunc,
    ) -> Self {
        CommandDefinition {
            name: name.to_string(),
            doc: doc.to_string(),
            flags_func,
            func,
        }
    }

    pub fn flags(&self) -> Vec<Flag> {
        (self.flags_func)()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn func(&self) -> &CommandFunc {
        &self.func
    }
}

pub struct CommandTable {
    commands: BTreeMap<String, CommandDefinition>,
}

impl CommandTable {
    pub fn new() -> Self {
        CommandTable {
            commands: BTreeMap::new(),
        }
    }
}

impl Deref for CommandTable {
    type Target = BTreeMap<String, CommandDefinition>;

    fn deref(&self) -> &Self::Target {
        &self.commands
    }
}

pub trait Register<FN, T> {
    fn register(&mut self, f: FN, name: &str, doc: &str);
}

// NoRepo commands.
impl<S, FN> Register<FN, (S,)> for CommandTable
where
    S: From<ParseOutput> + StructFlags,
    FN: Fn(S, &mut IO) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str) {
        let func = move |opts: ParseOutput, io: &mut IO| f(opts.into(), io);
        let func = CommandFunc::NoRepo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func);
        self.commands.insert(name.to_string(), def);
    }
}

// InferRepo commands.
impl<S, FN> Register<FN, ((), S)> for CommandTable
where
    S: From<ParseOutput> + StructFlags,
    FN: Fn(S, &mut IO, Option<Repo>) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str) {
        let func =
            move |opts: ParseOutput, io: &mut IO, repo: Option<Repo>| f(opts.into(), io, repo);
        let func = CommandFunc::InferRepo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func);
        self.commands.insert(name.to_string(), def);
    }
}

// Repo commands.
impl<S, FN> Register<FN, ((), (), S)> for CommandTable
where
    S: From<ParseOutput> + StructFlags,
    FN: Fn(S, &mut IO, Repo) -> Result<u8, DispatchError> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str) {
        let func = move |opts: ParseOutput, io: &mut IO, repo: Repo| f(opts.into(), io, repo);
        let func = CommandFunc::Repo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func);
        self.commands.insert(name.to_string(), def);
    }
}
