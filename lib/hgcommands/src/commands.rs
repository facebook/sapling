// Copyright Facebook, Inc. 2019

use clidispatch::command::CommandDefinition;
use clidispatch::dispatch::*;
use clidispatch::errors::DispatchError;
use clidispatch::io::IO;
use clidispatch::repo::Repo;
use cliparser::parser::ParseOutput;

use revisionstore::{DataPackStore, DataStore, IndexedLogDataStore, UnionDataStore};
use types::{Key, Node, RepoPathBuf};

#[allow(dead_code)]
pub fn create_dispatcher() -> Dispatcher {
    let mut dispatcher = Dispatcher::new();
    let root_command = root_command();
    let debugstore_command = debugstore_command();
    dispatcher.register(root_command, root);
    dispatcher.register(debugstore_command, debugstore);

    dispatcher
}

#[allow(dead_code)]
pub fn dispatch(dispatcher: &mut Dispatcher) -> Result<u8, DispatchError> {
    let args = args()?;

    dispatcher.dispatch(args)
}

fn root_command() -> CommandDefinition {
    let command = CommandDefinition::new("root")
        .add_flag((' ', "shared", "show root of the shared repo", false))
        .with_doc(
            r#"print the root (top) of the current working directory

    Print the root directory of the current repository.

    Returns 0 on success.

        "#,
        );

    command
}

pub struct RootCommand {
    shared: bool,
}

impl From<ParseOutput> for RootCommand {
    fn from(opts: ParseOutput) -> Self {
        let shared: bool = opts.pick("shared");

        RootCommand { shared }
    }
}

pub fn root(
    cmd: RootCommand,
    args: Vec<String>,
    io: &mut IO,
    repo: Repo,
) -> Result<u8, DispatchError> {
    if args.len() > 0 {
        return Err(DispatchError::InvalidArguments {
            command_name: "root".to_string(),
        }); // root doesn't support arguments
    }

    let shared = repo.sharedpath()?;

    let path = if cmd.shared {
        shared.unwrap_or(repo.path().to_owned())
    } else {
        repo.path().to_owned()
    };

    io.write(format!("{}\n", path.canonicalize()?.to_string_lossy()))?;
    Ok(0)
}

fn debugstore_command() -> CommandDefinition {
    let command = CommandDefinition::new("debugstore")
        .add_flag((' ', "content", "print out contents of blob", false))
        .with_doc(
            r#"Print out information about blob from store.
            hg debugstore filepath hash --content
            returns 0 on success
        "#,
        );

    command
}

pub struct DebugstoreCommand {
    content: bool,
}

impl From<ParseOutput> for DebugstoreCommand {
    fn from(opts: ParseOutput) -> Self {
        let content: bool = opts.pick("shared");

        DebugstoreCommand { content }
    }
}

pub fn debugstore(
    cmd: DebugstoreCommand,
    args: Vec<String>,
    io: &mut IO,
    repo: Repo,
) -> Result<u8, DispatchError> {
    if args.len() != 2 || !cmd.content {
        return Err(DispatchError::InvalidArguments {
            command_name: "debugstore".to_string(),
        }); // debugstore requires arguments
    }
    let config = repo.get_config();
    let cachepath = match config.get("remotefilelog", "cachepath") {
        Some(c) => c,
        None => return Err(DispatchError::ConfigIssue),
    };
    let reponame = match config.get("remotefilelog", "reponame") {
        Some(c) => c,
        None => return Err(DispatchError::ConfigIssue),
    };
    let cachepath = String::from_utf8_lossy(&cachepath[..]);
    let reponame = String::from_utf8_lossy(&reponame[..]);
    let fullpath = format!("{}/{}/packs", cachepath, reponame);
    let packstore = Box::new(DataPackStore::new(fullpath));
    let fullpath = format!("{}/{}/indexedlogdatastore", cachepath, reponame);
    let indexedstore = Box::new(IndexedLogDataStore::new(fullpath).unwrap());
    let mut unionstore: UnionDataStore<Box<dyn DataStore>> = UnionDataStore::new();
    unionstore.add(packstore);
    unionstore.add(indexedstore);
    let k = Key::new(
        RepoPathBuf::from_string(args[0].clone()).unwrap(),
        Node::from_str(&args[1]).unwrap(),
    );
    let content = unionstore.get(&k).unwrap();
    io.write(content)?;
    Ok(0)
}
