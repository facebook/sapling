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
use configloader::config::ConfigSet;
use repo::repo::Repo;
use workingcopy::workingcopy::WorkingCopy;

use crate::io::IO;
use crate::OptionalRepo;
use crate::ReqCtx;

pub enum CommandFunc {
    NoRepo(Box<dyn Fn(ParseOutput, &IO, &mut ConfigSet) -> Result<u8>>),
    OptionalRepo(Box<dyn Fn(ParseOutput, &IO, &mut OptionalRepo) -> Result<u8>>),
    Repo(Box<dyn Fn(ParseOutput, &IO, &mut Repo) -> Result<u8>>),
    WorkingCopy(Box<dyn Fn(ParseOutput, &IO, &mut Repo, &mut WorkingCopy) -> Result<u8>>),
}

pub struct CommandDefinition {
    aliases: String,
    doc: String,
    flags_func: fn() -> Vec<Flag>,
    func: CommandFunc,
    synopsis: Option<String>,
}

impl CommandDefinition {
    pub fn new(
        aliases: impl ToString,
        doc: impl ToString,
        flags_func: fn() -> Vec<Flag>,
        func: CommandFunc,
        synopsis: Option<impl ToString>,
    ) -> Self {
        CommandDefinition {
            aliases: aliases.to_string(),
            doc: doc.to_string(),
            flags_func,
            func,
            synopsis: synopsis.map(|s| s.to_string()),
        }
    }

    pub fn flags(&self) -> Vec<Flag> {
        (self.flags_func)()
    }

    pub fn aliases(&self) -> &str {
        &self.aliases
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

    pub fn main_alias(&self) -> &str {
        if let Some(name) = self.aliases.split('|').next() {
            name
        } else {
            ""
        }
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
    fn insert_aliases<'a>(&mut self, aliases: &'a str) {
        if !aliases.contains('|') {
            return;
        }
        for name in aliases.split('|') {
            self.alias.insert(name.to_string(), aliases.to_string());
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
    fn register(&mut self, f: FN, aliases: &str, doc: &str, synopsis: Option<&str>);
}

// OptionalRepo commands.
impl<S, FN> Register<FN, ((), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(ReqCtx<S>, &mut OptionalRepo) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, aliases: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(aliases);
        let func = move |opts: ParseOutput, io: &IO, repo: &mut OptionalRepo| {
            f(ReqCtx::new(opts, io.clone())?, repo)
        };
        let func = CommandFunc::OptionalRepo(Box::new(func));
        let def = CommandDefinition::new(aliases, doc, S::flags, func, synopsis);
        self.commands.insert(aliases.to_string(), def);
    }
}

// Repo commands.
impl<S, FN> Register<FN, ((), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(ReqCtx<S>, &mut Repo) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, aliases: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(aliases);
        let func = move |opts: ParseOutput, io: &IO, repo: &mut Repo| {
            f(ReqCtx::new(opts, io.clone())?, repo)
        };
        let func = CommandFunc::Repo(Box::new(func));
        let def = CommandDefinition::new(aliases, doc, S::flags, func, synopsis);
        self.commands.insert(aliases.to_string(), def);
    }
}

// NoRepo commands.
impl<S, FN> Register<FN, ((), (), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(ReqCtx<S>, &mut ConfigSet) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, aliases: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(aliases);
        let func = move |opts: ParseOutput, io: &IO, config: &mut ConfigSet| {
            f(ReqCtx::new(opts, io.clone())?, config)
        };
        let func = CommandFunc::NoRepo(Box::new(func));
        let def = CommandDefinition::new(aliases, doc, S::flags, func, synopsis);
        self.commands.insert(aliases.to_string(), def);
    }
}

// WorkingCopy commands.
impl<S, FN> Register<FN, ((), (), (), (), S)> for CommandTable
where
    S: TryFrom<ParseOutput, Error = anyhow::Error> + StructFlags,
    FN: Fn(ReqCtx<S>, &mut Repo, &mut WorkingCopy) -> Result<u8> + 'static,
{
    fn register(&mut self, f: FN, aliases: &str, doc: &str, synopsis: Option<&str>) {
        self.insert_aliases(aliases);
        let func =
            move |opts: ParseOutput, io: &IO, repo: &mut Repo, working_copy: &mut WorkingCopy| {
                f(ReqCtx::new(opts, io.clone())?, repo, working_copy)
            };
        let func = CommandFunc::WorkingCopy(Box::new(func));
        let def = CommandDefinition::new(aliases, doc, S::flags, func, synopsis);
        self.commands.insert(aliases.to_string(), def);
    }
}
