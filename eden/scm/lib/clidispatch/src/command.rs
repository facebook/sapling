/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::ops::Deref;

use anyhow::Result;
use cliparser::parser::Flag;
use cliparser::parser::ParseOutput;
use cliparser::parser::StructFlags;
use configparser::config::ConfigSet;
use repo::repo::Repo;

use crate::global_flags::HgGlobalOpts;
use crate::io::IO;
use crate::OptionalRepo;

pub enum CommandFunc {
    NoRepo(Box<dyn Fn(ParseOutput, &IO, &mut ConfigSet) -> Result<u8>>),
    NoRepoGlobalOpts(Box<dyn Fn(ParseOutput, &IO, &mut ConfigSet) -> Result<u8>>),
    OptionalRepo(Box<dyn Fn(ParseOutput, &IO, &mut OptionalRepo) -> Result<u8>>),
    OptionalRepoGlobalOpts(Box<dyn Fn(ParseOutput, &IO, &mut OptionalRepo) -> Result<u8>>),
    Repo(Box<dyn Fn(ParseOutput, &IO, &mut Repo) -> Result<u8>>),
}

pub struct CommandDefinition {
    name: String,
    doc: String,
    flags_func: fn() -> Vec<Flag>,
    func: CommandFunc,
    synopsis: Option<String>,
}

impl CommandDefinition {
    pub fn new(
        name: impl ToString,
        doc: impl ToString,
        flags_func: fn() -> Vec<Flag>,
        func: CommandFunc,
        synopsis: Option<impl ToString>,
    ) -> Self {
        CommandDefinition {
            name: name.to_string(),
            doc: doc.to_string(),
            flags_func,
            func,
            synopsis: synopsis.map(|s| s.to_string()),
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

    pub fn synopsis(&self) -> Option<&str> {
        self.synopsis.as_deref()
    }
}

#[derive(Default)]
pub struct CommandTable {
    commands: BTreeMap<String, CommandDefinition>,

    /// Alias name -> Command name.
    alias: BTreeMap<String, String>,
}

impl CommandTable {
    pub fn new() -> Self {
        Default::default()
    }

    /// Insert aliases to `alias` field.
    ///
    /// For example, `insert_aliases("config|cfg")` will insert
    /// `{"config": "config|cfg", "cfg": "config|cfg"}` to `alias`.
    fn insert_aliases<'a>(&mut self, names: &'a str) {
        if !names.contains("|") {
            return;
        }
        for name in names.split("|") {
            self.alias.insert(name.to_string(), names.to_string());
        }
    }

    /// Look up a command by name. Consider aliases.
    pub fn get(&self, name: &str) -> Option<&CommandDefinition> {
        let name = self.alias.get(name).map(AsRef::as_ref).unwrap_or(name);
        self.commands.get(name)
    }
}

impl Deref for CommandTable {
    type Target = BTreeMap<String, CommandDefinition>;

    fn deref(&self) -> &Self::Target {
        &self.commands
    }
}

pub trait Register<FN, T> {
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>);
}

// NoRepo commands.
impl<S, FN> Register<FN, (S,)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(S, &IO, &mut ConfigSet) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(name);
        let func = move |opts: ParseOutput, io: &IO, config: &mut ConfigSet| {
            f(opts.try_into()?, io, config)
        };
        let func = CommandFunc::NoRepo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func, synopsis);
        self.commands.insert(name.to_string(), def);
    }
}

// OptionalRepo commands.
impl<S, FN> Register<FN, ((), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(S, &IO, &mut OptionalRepo) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(name);
        let func = move |opts: ParseOutput, io: &IO, repo: &mut OptionalRepo| {
            f(opts.try_into()?, io, repo)
        };
        let func = CommandFunc::OptionalRepo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func, synopsis);
        self.commands.insert(name.to_string(), def);
    }
}

// Repo commands.
impl<S, FN> Register<FN, ((), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(S, &IO, &mut Repo) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(name);
        let func = move |opts: ParseOutput, io: &IO, repo: &mut Repo| f(opts.try_into()?, io, repo);
        let func = CommandFunc::Repo(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func, synopsis);
        self.commands.insert(name.to_string(), def);
    }
}

// NoRepoGlobalOpts commands.
impl<S, FN> Register<FN, ((), (), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(S, HgGlobalOpts, &IO, &mut ConfigSet) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(name);
        let func = move |opts: ParseOutput, io: &IO, config: &mut ConfigSet| {
            f(opts.clone().try_into()?, opts.try_into()?, io, config)
        };
        let func = CommandFunc::NoRepoGlobalOpts(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func, synopsis);
        self.commands.insert(name.to_string(), def);
    }
}

// OptionalRepoGlobalOpts commands.
impl<S, FN> Register<FN, ((), (), (), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(S, HgGlobalOpts, &IO, &mut OptionalRepo) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, name: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(name);
        let func = move |opts: ParseOutput, io: &IO, repo: &mut OptionalRepo| {
            f(opts.clone().try_into()?, opts.try_into()?, io, repo)
        };
        let func = CommandFunc::OptionalRepoGlobalOpts(Box::new(func));
        let def = CommandDefinition::new(name, doc, S::flags, func, synopsis);
        self.commands.insert(name.to_string(), def);
    }
}
