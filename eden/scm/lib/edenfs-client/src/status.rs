/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thrift_types::edenfs as eden;

use crate::path_relativizer::PathRelativizer;
use anyhow::{bail, ensure, Error, Result};
use byteorder::{BigEndian, ByteOrder};
use clidispatch::{errors::FallbackToPython, io::IO};
use crypto::{digest::Digest, sha2::Sha256};
use eden::client::EdenService;
use eden::{GetScmStatusParams, GetScmStatusResult, ScmFileStatus, ScmStatus};
#[cfg(unix)]
use fbthrift_socket::SocketTransport;
use std::collections::HashMap;
use std::default::Default;
use std::fs::read_link;
use std::fs::symlink_metadata;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use thrift_types::fb303_core::client::BaseService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use thrift_types::futures::future::TryFutureExt;
use tokio_core::reactor::Core;
#[cfg(unix)]
use tokio_uds::UnixStream;

/// Standalone status command for edenfs.
///
/// TODO: This does not match the Python implementation. Namely:
/// - No pager.
/// - Colors are hard-coded here.
/// - Global gitignore paths are hard-coded on edenfs side.
/// - No matcher support.
/// - No `--rev` support.
///
/// Consider integrating with `clidispatch` cleanly:
/// - Split into multiple functions.
///   - Communicate with EdenFS via Thrift.
///   - Wrap result with dirstate (Move to another crate?).
///   - Print status (Move to another crate).
/// - Avoid `print!` directly. Use `clidispatch::io` instead.
pub fn maybe_status_fastpath(
    repo_root: &Path,
    cwd: &Path,
    print_config: PrintConfig,
    io: &mut IO,
) -> Result<u8> {
    maybe_status_fastpath_internal(repo_root, cwd, print_config, io)
}

#[cfg(windows)]
fn maybe_status_fastpath_internal(
    repo_root: &Path,
    cwd: &Path,
    print_config: PrintConfig,
    io: &mut IO,
) -> Result<u8> {
    Err(FallbackToPython.into())
}

#[cfg(unix)]
fn maybe_status_fastpath_internal(
    repo_root: &Path,
    cwd: &Path,
    print_config: PrintConfig,
    io: &mut IO,
) -> Result<u8> {
    let mut core = Core::new().expect("Core creation failed");
    let handle = core.handle();

    // Look up the mount point name where Eden thinks this repository is
    // located.  This may be different from repo_root if a parent directory
    // of the Eden mount has been bind mounted to another location, resulting
    // in the Eden mount appearing at multiple separate locations.
    let eden_root = repo_root.join(".eden").join("root");
    let eden_root = read_link(eden_root).map_err(|_| FallbackToPython)?;
    let eden_root = eden_root
        .into_os_string()
        .into_string()
        .map_err(|_| FallbackToPython)?;

    // Look up Eden's socket address.
    let sock_addr = repo_root.join(".eden").join("socket");
    let sock_addr = read_link(sock_addr).map_err(|_| FallbackToPython)?;
    let sock = UnixStream::connect(&sock_addr, &handle).map_err(|_| FallbackToPython)?;

    let transport = SocketTransport::new(&handle, sock);
    let client = EdenService::new(BinaryProtocol, transport);
    let sock2 = UnixStream::connect(sock_addr, &handle).map_err(|_| FallbackToPython)?;

    let transport = SocketTransport::new(&handle, sock2);
    let fb303_client = BaseService::new(BinaryProtocol, transport);

    // TODO(mbolin): Run read_hg_dirstate() and core.run() in parallel.
    let dirstate_data = read_hg_dirstate(&repo_root)?;

    // If any of the files are present that should trigger the 'morestatus' extension, bail out of
    // the wrapper here and default to the Python implementation. D9025269 has a prototype
    // implementation of 'morestatus' in Rust, but we should gradually rewrite Mercurial in-place
    // and call out to it here rather than maintain a parallel implementation in the wrapper.
    let hg_dir = repo_root.join(".hg");
    if needs_morestatus_extension(&hg_dir, &dirstate_data.p2) {
        return Err(FallbackToPython.into());
    }

    let stdout = io::stdout();
    let use_color = should_colorize_output(&stdout);

    let status = get_status_helper(
        &mut core,
        &client,
        &fb303_client,
        &eden_root,
        dirstate_data.p1,
        print_config.status_types.ignored,
    )?;

    let relativizer = PathRelativizer::new(cwd.to_path_buf(), repo_root.to_path_buf());
    let relativizer = HgStatusPathRelativizer::new(print_config.root_relative, relativizer);
    print_config.print_status(
        &repo_root,
        &status.status,
        &dirstate_data,
        &relativizer,
        use_color,
        &mut io.output,
    )?;

    if let Ok(version) = status.version.parse::<u32>() {
        if use_color {
            let _ = io.write_err(BOLD);
        }
        // TODO: in the future we can have this look at some configuration that
        // we ship with the eden server, but for now, let's just hard code the
        // version check and advice.
        if version < 20180825 {
            let _ = io.write_err(
                "
IMPORTANT: Your running Eden server version is known to have issues importing
data from mercurial.  You should run `eden restart` at your earliest opportunity
to pick up the fix.\n",
            );
        } else if version == 20181023 {
            let _ = io.write_err(
                "
IMPORTANT: Your running Eden server version is known to have issues importing
data from mercurial.  You should run `eden restart && eden gc` at your earliest
opportunity to pick up the fix and fixup the cache.\n",
            );
        } else {
            use chrono::offset::TimeZone;
            let today = chrono::Local::today();

            // Construct a date object from the version number
            let day = version % 100;
            let month = (version % 10_000) / 100;
            let year = version / 10_000;
            let version_date = chrono::Local.ymd(year as i32, month, day);

            if today - version_date > chrono::Duration::days(45) {
                let _ = io.write_err(
                    "
Your running Eden server is more than 45 days old.  You should run
`eden restart` to update to the current release.\n",
                );
            }
        }
        if use_color {
            let _ = io.write_err(RESET);
        }
    }

    Ok(0)
}

const NULL_COMMIT: [u8; 20] = [0; 20];

fn needs_morestatus_extension(hg_dir: &Path, p2: &[u8; 20]) -> bool {
    if p2 != &NULL_COMMIT {
        return true;
    }

    for path in [
        PathBuf::from("bisect.state"),
        PathBuf::from("graftstate"),
        PathBuf::from("histedit-state"),
        PathBuf::from("merge/state"),
        PathBuf::from("rebasestate"),
        PathBuf::from("unshelverebasestate"),
        PathBuf::from("updatestate"),
    ]
    .iter()
    {
        if hg_dir.join(path).is_file() {
            return true;
        }
    }

    false
}

#[cfg(unix)]
fn should_colorize_output(stdout: &dyn AsRawFd) -> bool {
    let fd = stdout.as_raw_fd();
    let istty = unsafe { libc::isatty(fd as i32) } != 0;
    istty
}

#[cfg(not(unix))]
fn should_colorize_output(stdout: &io::Stdout) -> bool {
    false
}

fn is_unknown_method_error(error: &Error) -> bool {
    if let Some(eden::ErrorKind::EdenServiceGetScmStatusV2Error(
        eden::services::eden_service::GetScmStatusV2Exn::ApplicationException(ref e),
    )) = error.downcast_ref::<eden::ErrorKind>()
    {
        e.type_ == ApplicationExceptionErrorCode::UnknownMethod
    } else {
        false
    }
}

fn run_fallback_status(
    core: &mut Core,
    client: &Arc<impl EdenService>,
    fb303_client: &Arc<impl BaseService>,
    eden_root: &String,
    commit: CommitHash,
    ignored: bool,
) -> Result<GetScmStatusResult, Error> {
    match core.run(
        client
            .getScmStatus(&eden_root.as_bytes().to_vec(), ignored, &commit.to_vec())
            .compat(),
    ) {
        Ok(status) => {
            let version = core
                .run(
                    fb303_client
                        .getExportedValue("build_package_version")
                        .compat(),
                )
                .unwrap_or_else(|_| "".to_owned());

            Ok(GetScmStatusResult { status, version })
        }
        Err(error) => Err(error),
    }
}

fn get_status_helper(
    mut core: &mut Core,
    client: &Arc<impl EdenService>,
    fb303_client: &Arc<impl BaseService>,
    eden_root: &String,
    commit: CommitHash,
    ignored: bool,
) -> Result<GetScmStatusResult, Error> {
    let status = client
        .getScmStatusV2(&GetScmStatusParams {
            mountPoint: eden_root.as_bytes().to_vec(),
            commit: commit.to_vec(),
            listIgnored: ignored,
        })
        .compat();

    match core.run(status) {
        Ok(status) => Ok(status),
        Err(error) => {
            if is_unknown_method_error(&error) {
                run_fallback_status(
                    &mut core,
                    &client,
                    &fb303_client,
                    &eden_root,
                    commit,
                    ignored,
                )
            } else {
                Err(error)
            }
        }
    }
}

/// Config that determines how the output of `hg status` will be printed to the console.
pub struct PrintConfig {
    /// Determines which types of statuses will be displayed.
    pub status_types: PrintConfigStatusTypes,
    /// If true, the status will not be printed: only the path.
    pub no_status: bool,
    /// If true, for each file that was copied/moved, the source of copy/move will be printed
    /// below the destination.
    pub copies: bool,
    /// Termination character for the line: in practice this is '\0' or '\n'.
    pub endl: char,
    /// If true, paths are printed relative to the root of the repository; otherwise, they are
    /// printed relative to getcwd(2).
    pub root_relative: bool,
}

/// This struct covers the possible set of values for `hg status`. Used in conjunction with
/// PrintConfig: paths will only be included in the output of `hg status` if their corresponding
/// status is true in this struct.
pub struct PrintConfigStatusTypes {
    pub modified: bool,
    pub added: bool,
    pub removed: bool,
    pub deleted: bool,
    pub clean: bool,
    pub unknown: bool,
    pub ignored: bool,
}

/// Wrapper around an ordinary PathRelativizer that honors the --root-relative flag to `hg status`.
struct HgStatusPathRelativizer {
    relativizer: Option<PathRelativizer>,
}

impl HgStatusPathRelativizer {
    /// * `root_relative` true if --root-relative was specified.
    /// * `relativizer` comes from HgArgs.relativizer.
    pub fn new(root_relative: bool, relativizer: PathRelativizer) -> HgStatusPathRelativizer {
        let relativizer = match (root_relative, relativizer) {
            (false, r) => Some(r),
            _ => None,
        };
        HgStatusPathRelativizer { relativizer }
    }

    /// path is a normalized file path relative to repo_root. If root_relative is true, then the
    /// path that is returned will be relative to cwd.
    pub fn relativize(&self, path: &PathBuf) -> PathBuf {
        let out = match self.relativizer {
            Some(ref relativizer) => relativizer.relativize(path),
            None => path.clone(),
        };

        // Unfortunately, PathBuf does not have an is_empty() method:
        // https://github.com/rust-lang/rust/issues/30259.
        if !out.as_os_str().is_empty() {
            out
        } else {
            // In the rare event that the relativized path results in the empty string, print "."
            // instead so the user does not end up with an empty line.
            PathBuf::from(".")
        }
    }
}

/// Holds the result of parsing an argument list.
#[cfg(test)]
use telemetry::argparse::ParsedArgs;

impl PrintConfig {
    #[cfg(test)]
    fn new(command: &ParsedArgs) -> PrintConfig {
        // Note that if none of -mardcui is specified explicitly, then -mardu is assumed.
        let modified = command.is_present("modified");
        let added = command.is_present("added");
        let removed = command.is_present("removed");
        let deleted = command.is_present("deleted");
        let clean = command.is_present("clean");
        let unknown = command.is_present("unknown");
        let ignored = command.is_present("ignored");
        let status_types = if modified || added || removed || deleted || clean || unknown || ignored
        {
            PrintConfigStatusTypes {
                modified,
                added,
                removed,
                deleted,
                clean,
                unknown,
                ignored,
            }
        } else {
            PrintConfigStatusTypes {
                modified: true,
                added: true,
                removed: true,
                deleted: true,
                clean: false,
                unknown: true,
                ignored: false,
            }
        };

        let no_status = command.is_present("no-status");
        let endl = if command.is_present("print0") {
            '\0'
        } else {
            '\n'
        };
        PrintConfig {
            status_types,
            no_status,
            // Note that if --no-status is specified, then it disables --copies.
            copies: !no_status && command.is_present("copies"),
            endl,
            root_relative: command.hgplain || command.is_present("root-relative"),
        }
    }
    fn print_status<W: Write>(
        &self,
        repo_root: &Path,
        status: &ScmStatus,
        dirstate_data: &DirstateData,
        relativizer: &HgStatusPathRelativizer,
        use_color: bool,
        out: &mut W,
    ) -> Result<()> {
        let groups = group_entries(&repo_root, &status, &dirstate_data)?;
        let endl = self.endl;

        let mut print_group =
            |print_group, enabled: bool, group: &Vec<PathBuf>| -> Result<(), io::Error> {
                if !enabled {
                    return Ok(());
                }

                // `hg config | grep color` did not yield the entries for color.status listed on
                // https://www.mercurial-scm.org/wiki/ColorExtension. At Facebook, we seem to match
                // the defaults listed on the wiki page, except we don't change the background color.
                let (code, ansi_prefix) = match print_group {
                    PrintGroup::Modified => ("M ", format!("{}{}", BLUE, BOLD)),
                    PrintGroup::Added => ("A ", format!("{}{}", GREEN, BOLD)),
                    PrintGroup::Removed => ("R ", format!("{}{}", RED, BOLD)),
                    PrintGroup::Deleted => ("! ", format!("{}{}{}", CYAN, BOLD, UNDERLINE)),
                    PrintGroup::Unknown => ("? ", format!("{}{}{}", MAGENTA, BOLD, UNDERLINE)),
                    PrintGroup::Ignored => ("I ", format!("{}{}", BRIGHT_BLACK, BOLD)),
                    PrintGroup::Clean => ("C ", "".to_owned()),
                };
                let prefix = if self.no_status { "" } else { code };
                let (prefix, suffix) = if use_color {
                    (format!("{}{}", ansi_prefix, prefix), RESET.to_string())
                } else {
                    (prefix.to_owned(), "".to_owned())
                };

                for path in group {
                    write!(
                        out,
                        "{}{}{}{}",
                        prefix,
                        &relativizer.relativize(&path).display(),
                        suffix,
                        endl
                    )?;
                    if self.copies {
                        if let Some(ref p) = dirstate_data.copymap.get(path) {
                            write!(out, "  {}{}", &relativizer.relativize(p).display(), endl)?;
                        }
                    }
                }
                return Ok(());
            };

        print_group(
            PrintGroup::Modified,
            self.status_types.modified,
            &groups.modified,
        )?;
        print_group(PrintGroup::Added, self.status_types.added, &groups.added)?;
        print_group(
            PrintGroup::Removed,
            self.status_types.removed,
            &groups.removed,
        )?;
        print_group(
            PrintGroup::Deleted,
            self.status_types.deleted,
            &groups.deleted,
        )?;
        print_group(
            PrintGroup::Unknown,
            self.status_types.unknown,
            &groups.unknown,
        )?;
        print_group(
            PrintGroup::Ignored,
            self.status_types.ignored,
            &groups.ignored,
        )?;
        print_group(PrintGroup::Clean, self.status_types.clean, &groups.clean)?;
        return Ok(());
    }
}

const RED: &str = "\u{001B}[31m";
const BLUE: &str = "\u{001B}[34m";
const MAGENTA: &str = "\u{001B}[35m";
const GREEN: &str = "\u{001B}[32m";
const CYAN: &str = "\u{001B}[36m";
const BRIGHT_BLACK: &str = "\u{001B}[30;1m"; // Effectively grey.

const BOLD: &str = "\u{001B}[1m";
const UNDERLINE: &str = "\u{001b}[4m";
const RESET: &str = "\u{001B}[0m";

enum PrintGroup {
    Modified,
    Added,
    Removed,
    Deleted,
    Unknown,
    Ignored,
    Clean,
}

#[derive(Default)]
struct GroupedEntries {
    modified: Vec<PathBuf>,
    added: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    deleted: Vec<PathBuf>,
    unknown: Vec<PathBuf>,
    ignored: Vec<PathBuf>,
    clean: Vec<PathBuf>,
}

fn group_entries(
    repo_root: &Path,
    status: &ScmStatus,
    dirstate_data: &DirstateData,
) -> Result<GroupedEntries> {
    let mut result = GroupedEntries::default();
    let mut dirstates = dirstate_data.tuples.clone();
    for (path_str, status_code) in &status.entries {
        let path = Path::new(str::from_utf8(path_str)?);
        let dirstate = dirstates.remove(path);
        use self::DirstateDataStatus::*;
        let group = match (status_code.clone(), dirstate) {
            (ScmFileStatus::MODIFIED, Some(DirstateDataTuple { status: Remove, .. })) => {
                &mut result.removed
            }
            (ScmFileStatus::MODIFIED, _) => &mut result.modified,

            (ScmFileStatus::REMOVED, Some(DirstateDataTuple { status: Remove, .. })) => {
                &mut result.removed
            }
            (ScmFileStatus::REMOVED, _) => &mut result.deleted,

            (ScmFileStatus::ADDED, Some(DirstateDataTuple { status: Add, .. }))
            | (
                ScmFileStatus::ADDED,
                Some(DirstateDataTuple {
                    status: Normal,
                    merge_state: DirstateMergeState::OtherParent,
                    ..
                }),
            ) => &mut result.added,
            (ScmFileStatus::ADDED, _) => &mut result.unknown,

            (ScmFileStatus::IGNORED, Some(DirstateDataTuple { status: Add, .. })) => {
                &mut result.added
            }
            (ScmFileStatus::IGNORED, _) => &mut result.ignored,

            (ScmFileStatus(_), _) => unreachable!(
                "Illegal state: this should not be reachable \
                 once Thrift enums are translated as Rust enums."
            ),
        };
        group.push(path.to_path_buf());
    }

    for (path, tuple) in dirstates {
        match tuple.status {
            DirstateDataStatus::Merge => {
                if tuple.merge_state == DirstateMergeState::NotApplicable {
                    eprintln!(
                        "Unexpected Nonnormal file {} has a merge state of \
                         NotApplicable but is marked as 'needs merging'.",
                        path.display()
                    );
                } else {
                    result.modified.push(path)
                }
            }
            DirstateDataStatus::Add => match symlink_metadata(repo_root.join(&path)) {
                Ok(ref attr) if attr.is_dir() => eprintln!(
                    "Suspicious: dirstate tuple points to a directory: {}",
                    path.display()
                ),
                Ok(_) => result.added.push(path),
                Err(_) => result.deleted.push(path),
            },
            DirstateDataStatus::Remove => result.removed.push(path),
            DirstateDataStatus::Normal => continue,
            DirstateDataStatus::Unknown => continue,
        }
    }

    Ok(result)
}

struct DirstateReader {
    reader: BufReader<File>,
    sha256: Sha256,
}

impl DirstateReader {
    fn hashing_read(&mut self, buf: &mut [u8]) -> Result<(), io::Error> {
        self.reader.read_exact(buf)?;
        self.sha256.input(&buf);
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, io::Error> {
        let mut buf = [0; 1];
        self.hashing_read(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u16(&mut self) -> Result<u16, io::Error> {
        let mut buf = [0; 2];
        self.hashing_read(&mut buf)?;
        Ok(BigEndian::read_u16(&buf))
    }

    fn read_u32(&mut self) -> Result<u32, io::Error> {
        let mut buf = [0; 4];
        self.hashing_read(&mut buf)?;
        Ok(BigEndian::read_u32(&buf))
    }

    fn read_path(&mut self) -> Result<PathBuf> {
        let path_length = self.read_u16()?;

        let mut buf = vec![0; path_length as usize];
        self.reader.read_exact(&mut buf)?;
        self.sha256.input(&buf);

        Ok(Path::new(str::from_utf8(&buf)?).to_path_buf())
    }

    fn verify_checksum(&mut self) -> Result<(), io::Error> {
        let mut binary_checksum = [0; 32];
        self.reader.read_exact(&mut binary_checksum)?;

        let mut observed_digest = [0; 32];
        self.sha256.result(&mut observed_digest);

        if binary_checksum != observed_digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Checksum in .hg/dirstate did not match the hash of the file contents.",
            ));
        }

        let mut buf = [0; 1];
        match self.reader.read_exact(&mut buf) {
            // We expect that there should be nothing left to read after the checksum
            // has been read.
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
            // Some unexpected type of I/O error was returned.
            Err(e) => Err(e),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Suspicious data is present after the end of the checksum.",
            )),
        }
    }
}

fn read_hg_dirstate(repo_root: &Path) -> Result<DirstateData> {
    let dirstate = repo_root.join(".hg").join("dirstate");
    let mut reader = DirstateReader {
        reader: BufReader::new(File::open(dirstate)?),
        sha256: Sha256::new(),
    };

    let mut p1 = [0; 20];
    reader.hashing_read(&mut p1)?;
    let mut p2 = [0; 20];
    reader.hashing_read(&mut p2)?;

    let version = reader.read_u32()?;
    ensure!(version == 1, "Unsupported dirstate version: {}", version);

    let mut tuples: HashMap<PathBuf, DirstateDataTuple> = HashMap::new();
    let mut copymap: HashMap<PathBuf, PathBuf> = HashMap::new();

    loop {
        let header = reader.read_u8()?;
        match header {
            0x01 => {
                let status = match reader.read_u8()? {
                    b'n' => DirstateDataStatus::Normal,
                    b'm' => DirstateDataStatus::Merge,
                    b'r' => DirstateDataStatus::Remove,
                    b'a' => DirstateDataStatus::Add,
                    b'?' => DirstateDataStatus::Unknown,
                    value => bail!("Unknown status type (ASCII value): {}", value),
                };

                // The next four bytes compose an unsigned integer that corresponds to mode_t.
                // This implementation of `hg status` does not need it because it does not
                // currently support path arguments that would require us to do a directory
                // traversal that would have to consider the mode.
                let _mode = reader.read_u32()?;
                let merge_state = match reader.read_u8()? as i8 {
                    0 => DirstateMergeState::NotApplicable,
                    -1 => DirstateMergeState::BothParents,
                    -2 => DirstateMergeState::OtherParent,
                    value => bail!("Unknown merge type: {}", value),
                };
                let path = reader.read_path()?;
                tuples.insert(
                    path,
                    DirstateDataTuple {
                        status,
                        merge_state,
                    },
                );
            }
            0x02 => {
                let dest = reader.read_path()?;
                let source = reader.read_path()?;
                copymap.insert(dest, source);
            }
            0xFF => {
                reader.verify_checksum()?;
                break;
            }
            header => bail!("Unrecognized header byte: {:x}", header),
        };
    }

    Ok(DirstateData {
        p1,
        p2,
        tuples,
        copymap,
    })
}

type CommitHash = [u8; 20];

#[derive(Clone, Default)]
struct DirstateData {
    p1: CommitHash,
    p2: CommitHash,
    tuples: HashMap<PathBuf, DirstateDataTuple>,
    copymap: HashMap<PathBuf, PathBuf>,
}

#[derive(Clone)]
struct DirstateDataTuple {
    status: DirstateDataStatus,
    merge_state: DirstateMergeState,
}

#[derive(Clone, PartialEq)]
enum DirstateDataStatus {
    Normal,
    Merge,
    Remove,
    Add,
    Unknown,
}

#[derive(Clone, PartialEq)]
enum DirstateMergeState {
    NotApplicable,
    BothParents,
    OtherParent,
}

// TODO: Consider migrating from telemetry::hgargparse to cliparser for faster
// build and better OSS support.
#[cfg(test)]
mod test {
    use super::*;
    use std::collections::BTreeMap;
    use telemetry::hgargparse::{hg_parser, parse_args};
    use telemetry::test_utils::{generate_fixture, Fixture};

    #[derive(Default)]
    struct StatusTestCase<'a> {
        args: Vec<String>,
        p1: [u8; 20],
        p2: [u8; 20],
        entries: BTreeMap<Vec<u8>, ScmFileStatus>,
        dirstate_data_tuples: HashMap<PathBuf, DirstateDataTuple>,
        files: Vec<(&'a str, Fixture<'a>)>,
        use_color: bool,
        stdout: String,
    }

    /// This function is used to drive most of the tests. It runs PrintConfig.print_status(), so it
    /// focuses on exercising the display logic under various scenarios.
    fn test_status(test_case: StatusTestCase<'_>) {
        let repo_root = generate_fixture(test_case.files);
        let mut all_args: Vec<String> = vec![
            "--cwd".to_owned(),
            repo_root.path().to_str().unwrap().to_string(),
            "status".to_owned(),
        ];
        all_args.extend_from_slice(&test_case.args);
        let (hg_args, _handler) = parse_args(hg_parser(), &all_args[..]);
        let print_config = PrintConfig::new(&hg_args.command);

        let dirstate_data = DirstateData {
            p1: test_case.p1,
            p2: test_case.p2,
            tuples: test_case.dirstate_data_tuples,
            ..Default::default()
        };
        let status = ScmStatus {
            entries: test_case.entries,
            ..Default::default()
        };

        let relativizer = HgStatusPathRelativizer::new(
            print_config.root_relative,
            PathRelativizer::new(hg_args.cwd, repo_root.path().to_path_buf()),
        );
        let mut stdout: Vec<u8> = vec![];
        assert!(print_config
            .print_status(
                repo_root.path(),
                &status,
                &dirstate_data,
                &relativizer,
                test_case.use_color,
                &mut stdout
            )
            .is_ok());
        assert_eq!(test_case.stdout, str::from_utf8(&stdout).unwrap());
    }

    fn one_modified_file() -> BTreeMap<Vec<u8>, ScmFileStatus> {
        let mut entries = BTreeMap::new();
        entries.insert("file.txt".into(), ScmFileStatus::MODIFIED);
        entries
    }

    #[test]
    fn empty_status() {
        test_status(Default::default());
    }

    // XXX: PathRelativizer is problematic on OSX.
    #[cfg(target_os = "linux")]
    #[test]
    fn all_status_types() {
        let mut entries = BTreeMap::new();
        let mut dirstate_data_tuples = HashMap::new();

        entries.insert("added.txt".into(), ScmFileStatus::ADDED);
        dirstate_data_tuples.insert(
            PathBuf::from("added.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Add,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert("added_other_parent.txt".into(), ScmFileStatus::ADDED);
        dirstate_data_tuples.insert(
            PathBuf::from("added_other_parent.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Normal,
                merge_state: DirstateMergeState::OtherParent,
            },
        );

        entries.insert("unknown.txt".into(), ScmFileStatus::ADDED);
        dirstate_data_tuples.insert(
            PathBuf::from("unknown.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Normal,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert(
            "added_even_though_normally_ignored.txt".into(),
            ScmFileStatus::IGNORED,
        );
        dirstate_data_tuples.insert(
            PathBuf::from("added_even_though_normally_ignored.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Add,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert(
            "modified_and_marked_for_removal.txt".into(),
            ScmFileStatus::MODIFIED,
        );
        dirstate_data_tuples.insert(
            PathBuf::from("modified_and_marked_for_removal.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Remove,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert("removed.txt".into(), ScmFileStatus::REMOVED);
        dirstate_data_tuples.insert(
            PathBuf::from("removed.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Remove,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert(
            "removed_but_not_marked_for_removal.txt".into(),
            ScmFileStatus::REMOVED,
        );
        dirstate_data_tuples.insert(
            PathBuf::from("removed_but_not_marked_for_removal.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Normal,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert("ignored.txt".into(), ScmFileStatus::IGNORED);
        entries.insert("modified.txt".into(), ScmFileStatus::MODIFIED);

        // We have to slice [1..] to strip the leading newline.
        let no_arg_stdout = r#"
M modified.txt
A added.txt
A added_even_though_normally_ignored.txt
A added_other_parent.txt
R modified_and_marked_for_removal.txt
R removed.txt
! removed_but_not_marked_for_removal.txt
? unknown.txt
"#[1..]
            .to_string();
        test_status(StatusTestCase {
            entries: entries.clone(),
            dirstate_data_tuples: dirstate_data_tuples.clone(),
            stdout: no_arg_stdout,
            ..Default::default()
        });

        // We have to slice [1..] to strip the leading newline.
        let mardui_stdout = r#"
M modified.txt
A added.txt
A added_even_though_normally_ignored.txt
A added_other_parent.txt
R modified_and_marked_for_removal.txt
R removed.txt
! removed_but_not_marked_for_removal.txt
? unknown.txt
I ignored.txt
"#[1..]
            .to_string();
        test_status(StatusTestCase {
            args: vec!["-mardui".to_owned()],
            entries: entries.clone(),
            dirstate_data_tuples: dirstate_data_tuples.clone(),
            stdout: mardui_stdout,
            ..Default::default()
        });

        let mardui_color_stdout = concat!(
"\u{001B}[34m\u{001B}[1mM modified.txt\u{001B}[0m\n",
"\u{001B}[32m\u{001B}[1mA added.txt\u{001B}[0m\n",
"\u{001B}[32m\u{001B}[1mA added_even_though_normally_ignored.txt\u{001B}[0m\n",
"\u{001B}[32m\u{001B}[1mA added_other_parent.txt\u{001B}[0m\n",
"\u{001B}[31m\u{001B}[1mR modified_and_marked_for_removal.txt\u{001B}[0m\n",
"\u{001B}[31m\u{001B}[1mR removed.txt\u{001B}[0m\n",
"\u{001B}[36m\u{001B}[1m\u{001b}[4m! removed_but_not_marked_for_removal.txt\u{001B}[0m\n",
"\u{001B}[35m\u{001B}[1m\u{001b}[4m? unknown.txt\u{001B}[0m\n",
"\u{001B}[30;1m\u{001B}[1mI ignored.txt\u{001B}[0m\n",
);
        test_status(StatusTestCase {
            args: vec!["-mardui".to_owned()],
            entries: entries.clone(),
            dirstate_data_tuples: dirstate_data_tuples.clone(),
            use_color: true,
            stdout: mardui_color_stdout.to_string(),
            ..Default::default()
        });
    }

    // XXX: PathRelativizer is problematic on OSX.
    #[cfg(target_os = "linux")]
    #[test]
    fn no_status_flag() {
        test_status(StatusTestCase {
            args: vec!["--no-status".to_owned()],
            entries: one_modified_file(),
            stdout: "file.txt\n".to_owned(),
            ..Default::default()
        });

        test_status(StatusTestCase {
            args: vec!["-n".to_owned()],
            entries: one_modified_file(),
            stdout: "file.txt\n".to_owned(),
            ..Default::default()
        });
    }

    #[test]
    fn do_not_use_morestatus_if_p2_is_unset() {
        let files = vec![(".hg", Fixture::Dir)];
        let repo_root = generate_fixture(files);
        let p2 = [0_u8; 20];
        assert!(!needs_morestatus_extension(
            &repo_root.path().join(".hg"),
            &p2
        ));
    }

    #[test]
    fn use_morestatus_if_p2_is_set() {
        let files = vec![(".hg", Fixture::Dir)];
        let repo_root = generate_fixture(files);
        let p2 = [1_u8; 20];
        assert!(needs_morestatus_extension(
            &repo_root.path().join(".hg"),
            &p2
        ));
    }

    #[test]
    fn use_morestatus_if_histedit_file_exists() {
        let files = vec![
            (".hg", Fixture::Dir),
            (".hg/histedit-state", Fixture::File(b"")),
        ];
        let repo_root = generate_fixture(files);
        let p2 = [0_u8; 20];
        assert!(needs_morestatus_extension(
            &repo_root.path().join(".hg"),
            &p2
        ));
    }

    #[test]
    fn use_morestatus_if_merge_slash_state_file_exists() {
        let files = vec![
            (".hg", Fixture::Dir),
            (".hg/merge", Fixture::Dir),
            (".hg/merge/state", Fixture::File(b"")),
        ];
        let repo_root = generate_fixture(files);
        let p2 = [0_u8; 20];
        assert!(needs_morestatus_extension(
            &repo_root.path().join(".hg"),
            &p2
        ));
    }
}
