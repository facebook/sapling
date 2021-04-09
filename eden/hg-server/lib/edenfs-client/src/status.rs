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
use eden::client::EdenService;
use eden::{GetScmStatusParams, GetScmStatusResult, ScmFileStatus, ScmStatus};
#[cfg(unix)]
use fbthrift_socket::SocketTransport;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::default::Default;
use std::fs::read_link;
use std::fs::symlink_metadata;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use thrift_types::fb303_core::client::BaseService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
#[cfg(unix)]
use tokio::net::UnixStream;

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
    io: &IO,
) -> Result<u8> {
    let mut rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async { maybe_status_fastpath_internal(repo_root, cwd, print_config, io).await })
}

#[cfg(windows)]
async fn maybe_status_fastpath_internal(
    repo_root: &Path,
    cwd: &Path,
    print_config: PrintConfig,
    io: &IO,
) -> Result<u8> {
    Err(FallbackToPython.into())
}

#[cfg(unix)]
async fn maybe_status_fastpath_internal(
    repo_root: &Path,
    cwd: &Path,
    print_config: PrintConfig,
    io: &IO,
) -> Result<u8> {
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
    let sock = UnixStream::connect(&sock_addr)
        .await
        .map_err(|_| FallbackToPython)?;

    let transport = SocketTransport::new(sock);
    let client = EdenService::new(BinaryProtocol, transport);
    let sock2 = UnixStream::connect(sock_addr)
        .await
        .map_err(|_| FallbackToPython)?;

    let transport = SocketTransport::new(sock2);
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
        &client,
        &fb303_client,
        &eden_root,
        dirstate_data.p1,
        print_config.status_types.ignored,
    )
    .await?;

    let relativizer = PathRelativizer::new(cwd, repo_root);
    let relativizer = HgStatusPathRelativizer::new(print_config.root_relative, relativizer);
    let return_code = print_config.print_status(
        &repo_root,
        &status.status,
        &dirstate_data,
        &relativizer,
        use_color,
        io,
    )?;

    if let Ok(version) = status.version.parse::<u32>() {
        if use_color {
            let _ = io.write_err(BOLD);
        }
        // TODO: in the future we can have this look at some configuration that
        // we ship with the eden server to provide a version check and advice

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
`eden restart --graceful` to update to the current release.\n",
            );
        }

        if use_color {
            let _ = io.write_err(RESET);
        }
    }

    Ok(return_code)
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

fn is_unknown_method_error(error: &eden::errors::eden_service::GetScmStatusV2Error) -> bool {
    if let eden::errors::eden_service::GetScmStatusV2Error::ApplicationException(ref e) = error {
        e.type_ == ApplicationExceptionErrorCode::UnknownMethod
    } else {
        false
    }
}

async fn run_fallback_status(
    client: &Arc<impl EdenService>,
    fb303_client: &Arc<impl BaseService>,
    eden_root: &String,
    commit: CommitHash,
    ignored: bool,
) -> Result<GetScmStatusResult, Error> {
    match client
        .getScmStatus(&eden_root.as_bytes().to_vec(), ignored, &commit.to_vec())
        .await
    {
        Ok(status) => {
            let version = fb303_client
                .getExportedValue("build_package_version")
                .await
                .unwrap_or_else(|_| "".to_owned());

            Ok(GetScmStatusResult { status, version })
        }
        Err(error) => Err(error.into()),
    }
}

async fn get_status_helper(
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
        .await;

    match status {
        Ok(status) => Ok(status),
        Err(error) => {
            if is_unknown_method_error(&error) {
                run_fallback_status(&client, &fb303_client, &eden_root, commit, ignored).await
            } else {
                Err(error.into())
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
    pub fn relativize<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.relativize_impl(path.as_ref())
    }

    pub fn relativize_impl(&self, path: &Path) -> PathBuf {
        let out = match self.relativizer {
            Some(ref relativizer) => relativizer.relativize(path),
            None => path.to_path_buf(),
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

impl PrintConfig {
    fn print_status(
        &self,
        repo_root: &Path,
        status: &ScmStatus,
        dirstate_data: &DirstateData,
        relativizer: &HgStatusPathRelativizer,
        use_color: bool,
        io: &IO,
    ) -> Result<u8> {
        let groups = group_entries(&repo_root, &status, &dirstate_data, io)?;
        let endl = self.endl;

        let print_group =
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
                    io.write(format!(
                        "{}{}{}{}",
                        prefix,
                        &relativizer.relativize(&path).display(),
                        suffix,
                        endl
                    ))?;
                    if self.copies {
                        if let Some(ref p) = dirstate_data.copymap.get(path) {
                            io.write(format!(
                                "  {}{}",
                                &relativizer.relativize(p).display(),
                                endl
                            ))?;
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

        if status.errors.is_empty() {
            Ok(0)
        } else {
            io.write_err("Encountered errors computing status for some paths:\n")?;
            for (path_str, error) in &status.errors {
                let path = Path::new(str::from_utf8(path_str)?);
                io.write_err(format!(
                    "  {}: {}\n",
                    &relativizer.relativize(&path.to_path_buf()).display(),
                    error,
                ))?;
            }
            Ok(1)
        }
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
    io: &IO,
) -> Result<GroupedEntries> {
    let mut result = GroupedEntries::default();
    let mut dirstates = dirstate_data.tuples.clone();
    for (path_str, status_code) in &status.entries {
        let path_str = match str::from_utf8(path_str) {
            Ok(s) => s,
            Err(e) => {
                io.write_err(format!(
                    "skipping invalid utf-8 filename: {} ({})\n",
                    String::from_utf8_lossy(path_str),
                    e
                ))?;
                continue;
            }
        };
        let path = Path::new(path_str);
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

        let observed_digest: [u8; 32] = self.sha256.clone().result().into();

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
