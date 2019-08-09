// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::errors::DispatchError;
use crate::io::IO;
use crate::repo::Repo;
use cliparser::parser::{FlagDefinition, ParseOutput};

pub enum CommandType {
    NoRepo(Box<dyn Fn(ParseOutput, Vec<String>, &mut IO) -> Result<u8, DispatchError>>),
    InferRepo(
        Box<dyn Fn(ParseOutput, Vec<String>, &mut IO, Option<Repo>) -> Result<u8, DispatchError>>,
    ),
    Repo(Box<dyn Fn(ParseOutput, Vec<String>, &mut IO, Repo) -> Result<u8, DispatchError>>),
}

pub struct CommandDefinition {
    name: String,
    is_python: bool,
    doc: Option<String>,
    flags: Vec<FlagDefinition>,
}

impl CommandDefinition {
    pub fn new<S: Into<String>>(name: S) -> Self {
        CommandDefinition {
            name: name.into(),
            is_python: false,
            doc: None,
            flags: Vec::new(),
        }
    }

    pub fn with_doc<S: Into<String>>(mut self, doc: S) -> Self {
        self.doc = Some(doc.into());
        self
    }

    pub fn add_flag(mut self, def: FlagDefinition) -> Self {
        self.flags.push(def);
        self
    }

    pub fn mark_as_python(mut self) -> Self {
        self.is_python = true;
        self
    }

    pub fn flags(&self) -> &Vec<FlagDefinition> {
        &self.flags
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_python(&self) -> bool {
        self.is_python
    }

    pub fn doc(&self) -> &Option<String> {
        &self.doc
    }
}
