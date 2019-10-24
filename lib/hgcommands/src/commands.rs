// Copyright Facebook, Inc. 2019

use clidispatch::{
    command::{CommandTable, Register},
    errors,
    failure::Fallible,
    io::IO,
    repo::Repo,
};
use cliparser::define_flags;

use revisionstore::{
    CorruptionPolicy, DataPackStore, DataStore, IndexedLogDataStore, UnionDataStore,
};
use std::path::Path;
use types::{HgId, Key, RepoPathBuf};

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
    table.register(debugpython, "debugpython", "run python interpreter");
    table.register(debugargs, "debug-args", "print arguments received");
    table.register(
        debugindexedlogdump,
        "debugindexedlog-dump",
        "dump indexedlog data",
    );
    table.register(
        debugindexedlogrepair,
        "debugindexedlog-repair",
        "repair indexedlog log",
    );

    table
}

define_flags! {
    pub struct RootOpts {
        /// show root of the shared repo
        shared: bool,
    }

    pub struct DebugstoreOpts {
        /// print blob contents
        content: bool,

        #[arg]
        path: String,

        #[arg]
        hgid: String,
    }

    pub struct DebugPythonOpts {
        #[args]
        args: Vec<String>,
    }

    pub struct DebugArgsOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn root(opts: RootOpts, io: &mut IO, repo: Repo) -> Fallible<u8> {
    let path = if opts.shared {
        repo.shared_path()
    } else {
        repo.path()
    };

    io.write(format!(
        "{}\n",
        util::path::normalize_for_display(&path.to_string_lossy())
    ))?;
    Ok(0)
}

pub fn debugstore(opts: DebugstoreOpts, io: &mut IO, repo: Repo) -> Fallible<u8> {
    let path = RepoPathBuf::from_string(opts.path)?;
    let hgid = HgId::from_str(&opts.hgid)?;
    let config = repo.config();
    let cachepath = match config.get("remotefilelog", "cachepath") {
        Some(c) => c,
        None => return Err(errors::Abort("remotefilelog.cachepath is not set".into()).into()),
    };
    let reponame = match config.get("remotefilelog", "reponame") {
        Some(c) => c,
        None => return Err(errors::Abort("remotefilelog.reponame is not set".into()).into()),
    };
    let cachepath = String::from_utf8_lossy(&cachepath[..]);
    let reponame = String::from_utf8_lossy(&reponame[..]);
    let fullpath = format!("{}/{}/packs", cachepath, reponame);
    let packstore = Box::new(DataPackStore::new(fullpath, CorruptionPolicy::IGNORE));
    let fullpath = format!("{}/{}/indexedlogdatastore", cachepath, reponame);
    let indexedstore = Box::new(IndexedLogDataStore::new(fullpath).unwrap());
    let mut unionstore: UnionDataStore<Box<dyn DataStore>> = UnionDataStore::new();
    unionstore.add(packstore);
    unionstore.add(indexedstore);
    let k = Key::new(path, hgid);
    if let Some(content) = unionstore.get(&k)? {
        io.write(content)?;
    }
    Ok(0)
}

pub fn debugpython(opts: DebugPythonOpts, io: &mut IO) -> Fallible<u8> {
    let mut args = opts.args;
    args.insert(0, "hgpython".to_string());
    let mut interp = crate::HgPython::new(args.clone());
    Ok(interp.run_python(args, io))
}

pub fn debugargs(opts: DebugArgsOpts, io: &mut IO) -> Fallible<u8> {
    match io.write(format!("{:?}\n", opts.args)) {
        Ok(_) => Ok(0),
        Err(_) => Ok(255),
    }
}

pub fn debugindexedlogdump(opts: DebugArgsOpts, io: &mut IO) -> Fallible<u8> {
    for path in opts.args {
        let _ = io.write(format!("{}\n", path));
        let path = Path::new(&path);
        if let Ok(meta) = indexedlog::log::LogMetadata::read_file(path) {
            io.write(format!("Metadata File {:?}\n{:?}\n", path, meta))?;
        } else if path.is_dir() {
            // Treate it as Log.
            let log = indexedlog::log::Log::open(path, Vec::new())?;
            io.write(format!("Log Directory {:?}:\n{:#?}\n", path, log))?;
        } else if path.is_file() {
            // Treate it as Index.
            let idx = indexedlog::index::OpenOptions::new().open(path)?;
            io.write(format!("Index File {:?}\n{:?}\n", path, idx))?;
        } else {
            io.write_err(format!("Path {:?} is not a file or directory.\n\n", path))?;
        }
    }
    Ok(0)
}

pub fn debugindexedlogrepair(opts: DebugArgsOpts, io: &mut IO) -> Fallible<u8> {
    for path in opts.args {
        io.write(format!("Repairing {:?}\n", path))?;
        io.write(format!(
            "{}\n",
            indexedlog::log::OpenOptions::new().repair(path)?
        ))?;
        io.write("Done\n")?;
    }
    Ok(0)
}
