// Copyright Facebook, Inc. 2019

use clidispatch::command::{CommandTable, Register};
use clidispatch::errors::DispatchError;
use clidispatch::io::IO;
use clidispatch::repo::Repo;
use cliparser::define_flags;

use revisionstore::{DataPackStore, DataStore, IndexedLogDataStore, UnionDataStore};
use types::{Key, Node, RepoPathBuf};

#[allow(dead_code)]
/// Return the main command table including all Rust commands.
pub fn table() -> CommandTable {
    let mut table = CommandTable::new();
    table.register(
        root,
        "root",
        r#"print the root (top) of the current working directory

    Print the root directory of the current repository.

    Returns 0 on success."#,
    );
    table.register(
        debugstore,
        "debugstore",
        "print information about blobstore",
    );

    table
}

define_flags! {
    pub struct RootOpts {
        /// show root of the shared repo
        shared: bool,

        #[args]
        args: Vec<String>,
    }

    pub struct DebugstoreOpts {
        /// print blob contents
        content: bool,

        #[args]
        args: Vec<String>,
    }
}

pub fn root(opts: RootOpts, io: &mut IO, repo: Repo) -> Result<u8, DispatchError> {
    let args = opts.args;
    if args != vec!["root"] {
        return Err(DispatchError::InvalidArguments {
            command_name: "root".to_string(),
        }); // root doesn't support arguments
    }

    let shared = repo.sharedpath()?;

    let path = if opts.shared {
        shared.unwrap_or(repo.path().to_owned())
    } else {
        repo.path().to_owned()
    };

    io.write(format!(
        "{}\n",
        util::path::normalize_for_display(&path.to_string_lossy())
    ))?;
    Ok(0)
}

pub fn debugstore(opts: DebugstoreOpts, io: &mut IO, repo: Repo) -> Result<u8, DispatchError> {
    let args = opts.args;
    if args.len() != 2 || !opts.content {
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
