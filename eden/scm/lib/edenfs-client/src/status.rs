/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::read_link;
use std::fs::symlink_metadata;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use clidispatch::errors::FallbackToPython;
use clidispatch::io::CanColor;
use clidispatch::io::IO;
use eden::GetScmStatusParams;
use eden::GetScmStatusResult;
use eden::ScmFileStatus;
use eden::ScmStatus;
#[cfg(unix)]
use fbthrift_socket::SocketTransport;
use sha2::Digest;
use sha2::Sha256;
use status::StatusBuilder;
use thrift_types::edenfs as eden;
use thrift_types::edenfs::client::EdenService;
use thrift_types::fb303_core::client::BaseService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
#[cfg(unix)]
use tokio_uds_compat::UnixStream;
use types::path::RepoPathRelativizer;
use types::RepoPath;
use types::RepoPathBuf;

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
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(maybe_status_fastpath_internal(
        repo_root,
        cwd,
        print_config,
        io,
    ))
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
    let client = <dyn EdenService>::new(BinaryProtocol, transport);
    let sock2 = UnixStream::connect(sock_addr)
        .await
        .map_err(|_| FallbackToPython)?;

    let transport = SocketTransport::new(sock2);
    let fb303_client = <dyn BaseService>::new(BinaryProtocol, transport);

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
    let use_color = stdout.can_color();

    let status = get_status_helper(
        &client,
        &fb303_client,
        &eden_root,
        dirstate_data.p1,
        print_config.status_types.ignored,
    )
    .await?;

    let status_output = group_entries(repo_root, &status.status, &dirstate_data, io)?;
    let relativizer = RepoPathRelativizer::new(cwd, repo_root);
    let relativizer = HgStatusPathRelativizer::new(print_config.root_relative, relativizer);
    print_config.print_status(
        &status_output,
        &dirstate_data.copymap,
        &relativizer,
        use_color,
        io,
    )?;
    let return_code = print_errors(&status.status, io)?;

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
    ] {
        if hg_dir.join(path).is_file() {
            return true;
        }
    }

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

            Ok(GetScmStatusResult {
                status,
                version,
                ..Default::default()
            })
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
            ..Default::default()
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
    relativizer: Option<RepoPathRelativizer>,
}

impl HgStatusPathRelativizer {
    /// * `root_relative` true if --root-relative was specified.
    /// * `relativizer` comes from HgArgs.relativizer.
    pub fn new(root_relative: bool, relativizer: RepoPathRelativizer) -> HgStatusPathRelativizer {
        let relativizer = match (root_relative, relativizer) {
            (false, r) => Some(r),
            _ => None,
        };
        HgStatusPathRelativizer { relativizer }
    }

    /// Returns a String that is suitable for display to the user.
    ///
    /// If `root_relative` is true, the path returned will be relative to the working directory.
    pub fn relativize(&self, repo_path: &RepoPath) -> String {
        let out = match self.relativizer {
            Some(ref relativizer) => relativizer.relativize(repo_path),
            None => repo_path.to_string(),
        };

        if !out.is_empty() {
            out
        } else {
            // In the rare event that the relativized path results in the empty string, print "."
            // instead so the user does not end up with an empty line.
            String::from(".")
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

    fn print_status(
        &self,
        status: &status::Status,
        copymap: &HashMap<RepoPathBuf, RepoPathBuf>,
        relativizer: &HgStatusPathRelativizer,
        use_color: bool,
        io: &IO,
    ) -> Result<()> {
        let endl = self.endl;

        let print_group = |
            print_group,
            enabled: bool,
            group: &mut dyn Iterator<Item = &RepoPathBuf>,
        | -> Result<(), io::Error> {
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

            let mut group = group.collect::<Vec<_>>();
            group.sort();
            for path in group {
                io.write(format!(
                    "{}{}{}{}",
                    prefix,
                    &relativizer.relativize(path),
                    suffix,
                    endl
                ))?;
                if self.copies {
                    if let Some(p) = copymap.get(path) {
                        io.write(format!("  {}{}", &relativizer.relativize(p), endl))?;
                    }
                }
            }
            Ok(())
        };

        print_group(
            PrintGroup::Modified,
            self.status_types.modified,
            &mut status.modified(),
        )?;
        print_group(
            PrintGroup::Added,
            self.status_types.added,
            &mut status.added(),
        )?;
        print_group(
            PrintGroup::Removed,
            self.status_types.removed,
            &mut status.removed(),
        )?;
        print_group(
            PrintGroup::Deleted,
            self.status_types.deleted,
            &mut status.deleted(),
        )?;
        print_group(
            PrintGroup::Unknown,
            self.status_types.unknown,
            &mut status.unknown(),
        )?;
        print_group(
            PrintGroup::Ignored,
            self.status_types.ignored,
            &mut status.ignored(),
        )?;
        print_group(
            PrintGroup::Clean,
            self.status_types.clean,
            &mut status.clean(),
        )?;

        Ok(())
    }
}

fn print_errors(raw_status: &ScmStatus, io: &IO) -> Result<u8> {
    if raw_status.errors.is_empty() {
        Ok(0)
    } else {
        io.write_err("Encountered errors computing status for some paths:\n")?;
        for (path_str, error) in &raw_status.errors {
            let path = RepoPath::from_utf8(path_str)?;
            io.write_err(format!("  {}: {}\n", path, error))?;
        }
        Ok(1)
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

fn group_entries(
    repo_root: &Path,
    status: &ScmStatus,
    dirstate_data: &DirstateData,
    io: &IO,
) -> Result<status::Status> {
    let mut modified = vec![];
    let mut added = vec![];
    let mut removed = vec![];
    let mut deleted = vec![];
    let mut unknown = vec![];
    let mut ignored = vec![];
    let clean = vec![];

    let mut dirstates = dirstate_data.tuples.clone();
    for (path_str, status_code) in &status.entries {
        let path = match RepoPath::from_utf8(path_str) {
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
        let dirstate = dirstates.remove(path);
        use self::DirstateDataStatus::*;
        let group = match (status_code.clone(), dirstate) {
            (ScmFileStatus::MODIFIED, Some(DirstateDataTuple { status: Remove, .. })) => {
                &mut removed
            }
            (ScmFileStatus::MODIFIED, _) => &mut modified,

            (ScmFileStatus::REMOVED, Some(DirstateDataTuple { status: Remove, .. })) => {
                &mut removed
            }
            (ScmFileStatus::REMOVED, _) => &mut deleted,

            (ScmFileStatus::ADDED, Some(DirstateDataTuple { status: Add, .. }))
            | (
                ScmFileStatus::ADDED,
                Some(DirstateDataTuple {
                    status: Normal,
                    merge_state: DirstateMergeState::OtherParent,
                    ..
                }),
            ) => &mut added,
            (ScmFileStatus::ADDED, _) => &mut unknown,

            (ScmFileStatus::IGNORED, Some(DirstateDataTuple { status: Add, .. })) => &mut added,
            (ScmFileStatus::IGNORED, _) => &mut ignored,

            (ScmFileStatus(_), _) => unreachable!(
                "Illegal state: this should not be reachable \
                 once Thrift enums are translated as Rust enums."
            ),
        };
        group.push(path.to_owned());
    }

    for (path, tuple) in dirstates {
        match tuple.status {
            DirstateDataStatus::Merge => {
                if tuple.merge_state == DirstateMergeState::NotApplicable {
                    eprintln!(
                        "Unexpected Nonnormal file {} has a merge state of \
                         NotApplicable but is marked as 'needs merging'.",
                        path
                    );
                } else {
                    modified.push(path)
                }
            }
            DirstateDataStatus::Add => match symlink_metadata(repo_root.join(path.as_str())) {
                Ok(ref attr) if attr.is_dir() => {
                    eprintln!("Suspicious: dirstate tuple points to a directory: {}", path)
                }
                Ok(_) => added.push(path),
                Err(_) => deleted.push(path),
            },
            DirstateDataStatus::Remove => removed.push(path),
            DirstateDataStatus::Normal => continue,
            DirstateDataStatus::Unknown => continue,
        }
    }

    let status = StatusBuilder::new()
        .modified(modified)
        .added(added)
        .removed(removed)
        .deleted(deleted)
        .unknown(unknown)
        .ignored(ignored)
        .clean(clean)
        .build();
    Ok(status)
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

    fn read_path(&mut self) -> Result<RepoPathBuf> {
        let path_length = self.read_u16()?;

        let mut buf = vec![0; path_length as usize];
        self.reader.read_exact(&mut buf)?;
        self.sha256.input(&buf);

        Ok(RepoPathBuf::from_utf8(buf)?)
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

    let mut tuples: HashMap<RepoPathBuf, DirstateDataTuple> = HashMap::new();
    let mut copymap: HashMap<RepoPathBuf, RepoPathBuf> = HashMap::new();

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
    tuples: HashMap<RepoPathBuf, DirstateDataTuple>,
    copymap: HashMap<RepoPathBuf, RepoPathBuf>,
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
    use std::collections::BTreeMap;

    use telemetry::hgargparse::hg_parser;
    use telemetry::hgargparse::parse_args;
    use telemetry::test_utils::generate_fixture;
    use telemetry::test_utils::Fixture;

    use super::*;

    fn repo_path_buf(path: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(path.to_string()).unwrap()
    }

    #[derive(Default)]
    struct StatusTestCase<'a> {
        args: Vec<String>,
        p1: [u8; 20],
        p2: [u8; 20],
        entries: BTreeMap<Vec<u8>, ScmFileStatus>,
        errors: BTreeMap<Vec<u8>, String>,
        dirstate_data_tuples: HashMap<RepoPathBuf, DirstateDataTuple>,
        files: Vec<(&'a str, Fixture<'a>)>,
        use_color: bool,
        stdout: String,
        stderr: String,
        return_code: u8,
    }

    /// This function is used to drive most of the tests. It runs PrintConfig.print_status(), so it
    /// focuses on exercising the display logic under various scenarios.
    fn test_status(test_case: StatusTestCase<'_>) {
        let repo_root = generate_fixture(test_case.files);
        let repo_root_path = repo_root.path().canonicalize().unwrap();
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
            errors: test_case.errors,
            ..Default::default()
        };

        let relativizer = HgStatusPathRelativizer::new(
            print_config.root_relative,
            RepoPathRelativizer::new(hg_args.cwd, repo_root_path),
        );
        let tin = "".as_bytes();
        let tout = Vec::new();
        let terr = Vec::new();
        let mut io = IO::new(tin, tout, Some(terr));
        let grouped = group_entries(repo_root.path(), &status, &dirstate_data, &io).unwrap();
        print_config
            .print_status(
                &grouped,
                &dirstate_data.copymap,
                &relativizer,
                test_case.use_color,
                &mut io,
            )
            .unwrap();
        let return_code = print_errors(&status, &io).unwrap();
        let actual_output =
            io.with_output(|o| o.as_any().downcast_ref::<Vec<u8>>().unwrap().clone());
        assert_eq!(str::from_utf8(&actual_output).unwrap(), test_case.stdout);
        let actual_error = io.with_error(|e| {
            e.as_ref()
                .unwrap()
                .as_any()
                .downcast_ref::<Vec<u8>>()
                .unwrap()
                .clone()
        });
        assert_eq!(str::from_utf8(&actual_error).unwrap(), test_case.stderr);
        assert_eq!(return_code, test_case.return_code);
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
            repo_path_buf("added.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Add,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert("added_other_parent.txt".into(), ScmFileStatus::ADDED);
        dirstate_data_tuples.insert(
            repo_path_buf("added_other_parent.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Normal,
                merge_state: DirstateMergeState::OtherParent,
            },
        );

        entries.insert("unknown.txt".into(), ScmFileStatus::ADDED);
        dirstate_data_tuples.insert(
            repo_path_buf("unknown.txt"),
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
            repo_path_buf("added_even_though_normally_ignored.txt"),
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
            repo_path_buf("modified_and_marked_for_removal.txt"),
            DirstateDataTuple {
                status: DirstateDataStatus::Remove,
                merge_state: DirstateMergeState::NotApplicable,
            },
        );

        entries.insert("removed.txt".into(), ScmFileStatus::REMOVED);
        dirstate_data_tuples.insert(
            repo_path_buf("removed.txt"),
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
            repo_path_buf("removed_but_not_marked_for_removal.txt"),
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

    #[test]
    fn status_with_errors() {
        let mut entries = BTreeMap::new();
        entries.insert("unknown.txt".into(), ScmFileStatus::ADDED);
        entries.insert("modified.txt".into(), ScmFileStatus::MODIFIED);

        let mut errors = BTreeMap::new();
        errors.insert(
            "src/lib".into(),
            "unable to fetch directory data: connection reset".into(),
        );

        let color_stdout = concat!(
            "\u{001B}[34m\u{001B}[1mM modified.txt\u{001B}[0m\n",
            "\u{001B}[35m\u{001B}[1m\u{001b}[4m? unknown.txt\u{001B}[0m\n",
        );
        let src_lib_path = Path::new("src").join("lib");
        let stderr = format!(
            "{}\n  {}: {}\n",
            "Encountered errors computing status for some paths:",
            src_lib_path.display(),
            "unable to fetch directory data: connection reset",
        );
        test_status(StatusTestCase {
            entries,
            errors,
            use_color: true,
            stdout: color_stdout.to_string(),
            stderr: stderr.to_string(),
            return_code: 1,
            ..Default::default()
        });
    }

    #[test]
    fn status_with_invalid_utf8() {
        let mut entries = BTreeMap::new();
        entries.insert(b"\xb0Z\xd0J\x91\x7f.INFO".to_vec(), ScmFileStatus::ADDED);
        entries.insert(b"modified.txt".to_vec(), ScmFileStatus::MODIFIED);
        let errors = BTreeMap::new();
        let stdout = "M modified.txt\n";
        let stderr = "skipping invalid utf-8 filename: �Z�J�\u{7f}.INFO (Failed to parse to Utf8: \"�Z�J�\\u{7f}.INFO\". invalid utf-8 sequence of 1 bytes from index 0)\n";
        test_status(StatusTestCase {
            entries,
            errors,
            use_color: false,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            return_code: 0,
            ..Default::default()
        });
    }
}
