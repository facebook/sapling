/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::{
    command::{CommandTable, Register},
    errors,
    failure::Fallible,
    io::IO,
    repo::Repo,
};
use cliparser::define_flags;

use blackbox::event::Event;
use edenapi::{Config as EdenApiConfig, EdenApi, EdenApiCurlClient};
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
    table.register(dump_trace, "dump-trace", "export tracing information");

    table.register(
        debugstore,
        "debugstore",
        "print information about blobstore",
    );
    table.register(debugpython, "debugpython|debugpy", "run python interpreter");
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
    table.register(
        debughttp,
        "debughttp",
        "check whether api server is reachable",
    );

    table
}

define_flags! {
    pub struct RootOpts {
        /// show root of the shared repo
        shared: bool,
    }

    pub struct DumpTraceOpts {
        /// time range
        #[short('t')]
        time_range: String = "since 15 minutes ago",

        /// blackbox session id (overrides --time-range)
        #[short('s')]
        session_id: i64,

        /// output path (.txt, .json, .json.gz, .spans.json)
        #[short('o')]
        output_path: String,
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
        /// modules to trace (ex. 'edenscm.* subprocess import')
        trace: String,

        #[args]
        args: Vec<String>,
    }

    pub struct DebugArgsOpts {
        #[args]
        args: Vec<String>,
    }

    pub struct DebugHttpOpts {}
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

pub fn dump_trace(opts: DumpTraceOpts, io: &mut IO, _repo: Repo) -> Fallible<u8> {
    let filter = if opts.session_id != 0 {
        blackbox::IndexFilter::SessionId(opts.session_id as u64)
    } else if let Some(range) = hgtime::HgTime::parse_range(&opts.time_range) {
        // Blackbox uses milliseconds. HgTime uses seconds.
        let ratio = 1000;
        blackbox::IndexFilter::Time(
            range.start.unixtime.saturating_mul(ratio),
            range.end.unixtime.saturating_mul(ratio),
        )
    } else {
        return Err(errors::Abort("both --time-range and --session-id are invalid".into()).into());
    };

    let entries = {
        let blackbox = blackbox::SINGLETON.lock();
        blackbox.filter::<Event>(filter, None)
    };

    let mut tracing_data_list = Vec::new();
    for entry in entries {
        if let Event::TracingData { serialized } = entry.data {
            if let Ok(uncompressed) = zstd::stream::decode_all(&serialized.0[..]) {
                if let Ok(data) = mincode::deserialize(&uncompressed) {
                    tracing_data_list.push(data)
                }
            }
        }
    }
    let merged = tracing_collector::TracingData::merge(tracing_data_list);

    crate::run::write_trace(io, &opts.output_path, &merged)?;

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
    let mut interp = crate::HgPython::new(&args);
    if !opts.trace.is_empty() {
        // Setup tracing via edenscm.traceimport
        let _ = interp.setup_tracing(opts.trace.clone());
    }
    Ok(interp.run_python(&args, io))
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

pub fn debughttp(_opts: DebugHttpOpts, io: &mut IO, repo: Repo) -> Fallible<u8> {
    let config = EdenApiConfig::from_hg_config(repo.config())?;
    let client = EdenApiCurlClient::new(config)?;
    let hostname = client.hostname()?;
    io.write(format!("successfully connected to: {}\n", hostname))?;
    Ok(0)
}
