// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::errors::DispatchError;
use crate::io::IO;
use crate::repo::Repo;
use cliparser::parser::{Flag, ParseOutput};

pub enum CommandFunc {
    NoRepo(Box<dyn Fn(ParseOutput, &mut IO) -> Result<u8, DispatchError>>),
    InferRepo(Box<dyn Fn(ParseOutput, &mut IO, Option<Repo>) -> Result<u8, DispatchError>>),
    Repo(Box<dyn Fn(ParseOutput, &mut IO, Repo) -> Result<u8, DispatchError>>),
}

pub struct CommandDefinition {
    name: String,
    doc: String,
    flags: Vec<Flag>,
}

impl CommandDefinition {
    pub fn new(name: impl ToString) -> Self {
        CommandDefinition {
            name: name.to_string(),
            doc: String::new(),
            flags: Vec::new(),
        }
    }

    pub fn with_doc(mut self, doc: impl ToString) -> Self {
        self.doc = doc.to_string();
        self
    }

    pub fn add_flag(mut self, def: impl Into<Flag>) -> Self {
        self.flags.push(def.into());
        self
    }

    pub fn flags(&self) -> &Vec<Flag> {
        &self.flags
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }
}
