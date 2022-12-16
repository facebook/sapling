/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::symlink_metadata;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use ::io::IsTty;
use ::io::IO;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use eden::GetScmStatusParams;
use eden::GetScmStatusResult;
use eden::ScmFileStatus;
use eden::ScmStatus;
use fbthrift_socket::SocketTransport;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;
use status::needs_morestatus_extension;
use status::StatusBuilder;
use thrift_types::edenfs as eden;
use thrift_types::edenfs::client::EdenService;
use thrift_types::fbthrift::binary_protocol::BinaryProtocol;
use tokio_uds_compat::UnixStream;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

#[derive(Debug, thiserror::Error)]
#[error("")]
pub struct OperationNotSupported;

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
    repo_dot_path: &Path,
    io: &IO,
    list_ignored: bool,
) -> Result<(status::Status, HashMap<RepoPathBuf, RepoPathBuf>)> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(maybe_status_fastpath_internal(
        repo_dot_path,
        io,
        list_ignored,
    ))
}

async fn get_socket_transport(sock_path: &Path) -> Result<SocketTransport<UnixStream>> {
    let sock = UnixStream::connect(&sock_path).await?;
    Ok(SocketTransport::new(sock))
}

pub fn get_status(repo_root: &Path) -> Result<GetScmStatusResult> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(get_status_internal(repo_root))
}

async fn get_status_internal(repo_root: &Path) -> Result<GetScmStatusResult> {
    let eden_config = EdenConfig::from_root(repo_root)?;

    let transport = get_socket_transport(&eden_config.socket).await?;
    let client = <dyn EdenService>::new(BinaryProtocol, transport);

    let dirstate_data = read_hg_dirstate(eden_config.root.as_ref())?;

    get_status_helper(&client, &eden_config.root, dirstate_data.p1, false).await
}

#[derive(Deserialize)]
struct EdenConfig {
    root: String,
    socket: PathBuf,
}

impl EdenConfig {
    fn from_root(root: &Path) -> Result<Self> {
        let dot_eden = root.join(".eden");

        // Look up the mount point name where Eden thinks this repository is
        // located.  This may be different from repo_root if a parent directory
        // of the Eden mount has been bind mounted to another location, resulting
        // in the Eden mount appearing at multiple separate locations.

        // Windows uses a toml .eden/config file due to lack of symlink support.
        if cfg!(windows) {
            let toml_path = dot_eden.join("config");

            match util::file::read_to_string(&toml_path) {
                Ok(toml_contents) => {
                    #[derive(Deserialize)]
                    struct Outer {
                        #[serde(rename = "Config")]
                        config: EdenConfig,
                    }

                    let outer: Outer = toml::from_str(&toml_contents)?;
                    return Ok(outer.config);
                }
                // Fallthrough and try symlinks just in case.
                Err(err) if err.is_not_found() => {}
                Err(err) => return Err(err.into()),
            }
        }

        let root = util::file::read_link(dot_eden.join("root"))?
            .into_os_string()
            .into_string()
            .map_err(|path| anyhow!("couldn't stringify path {:?}", path))?;
        Ok(Self {
            root,
            socket: util::file::read_link(dot_eden.join("socket"))?,
        })
    }
}

async fn maybe_status_fastpath_internal(
    repo_dot_path: &Path,
    io: &IO,
    list_ignored: bool,
) -> Result<(status::Status, HashMap<RepoPathBuf, RepoPathBuf>)> {
    let repo_root = match repo_dot_path.parent() {
        Some(p) => p,
        None => bail!("invalid dot dir {}", repo_dot_path.display()),
    };

    let eden_config = EdenConfig::from_root(repo_root).map_err(|_| OperationNotSupported)?;

    let transport = get_socket_transport(&eden_config.socket)
        .await
        .map_err(|_| OperationNotSupported)?;
    let client = <dyn EdenService>::new(BinaryProtocol, transport);

    // TODO(mbolin): Run read_hg_dirstate() and core.run() in parallel.
    let dirstate_data = read_hg_dirstate(eden_config.root.as_ref())?;

    // If any of the files are present that should trigger the 'morestatus' extension, bail out of
    // the wrapper here and default to the Python implementation. D9025269 has a prototype
    // implementation of 'morestatus' in Rust, but we should gradually rewrite Mercurial in-place
    // and call out to it here rather than maintain a parallel implementation in the wrapper.
    if needs_morestatus_extension(
        repo_dot_path,
        if HgId::from(dirstate_data.p2).is_null() {
            1
        } else {
            2
        },
    ) {
        return Err(OperationNotSupported.into());
    }

    let use_color = io.output().can_color();

    let status =
        get_status_helper(&client, &eden_config.root, dirstate_data.p1, list_ignored).await?;

    let status_output = group_entries(&repo_root, &status.status, &dirstate_data, io)?;
    let copymap = dirstate_data.copymap;
    print_errors(&status.status, io)?;

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

    Ok((status_output, copymap))
}

async fn get_status_helper(
    client: &Arc<impl EdenService>,
    eden_root: &String,
    commit: CommitHash,
    ignored: bool,
) -> Result<GetScmStatusResult, Error> {
    client
        .getScmStatusV2(&GetScmStatusParams {
            mountPoint: eden_root.as_bytes().to_vec(),
            commit: commit.to_vec(),
            listIgnored: ignored,
            ..Default::default()
        })
        .await
        .map_err(|err| err.into())
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

const BOLD: &str = "\u{001B}[1m";
const RESET: &str = "\u{001B}[0m";

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
        self.sha256.update(&buf);
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
        self.sha256.update(&buf);

        Ok(RepoPathBuf::from_utf8(buf)?)
    }

    fn verify_checksum(&mut self) -> Result<(), io::Error> {
        let mut binary_checksum = [0; 32];
        self.reader.read_exact(&mut binary_checksum)?;

        let observed_digest: [u8; 32] = self.sha256.clone().finalize().into();

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
    let ident = identity::must_sniff_dir(repo_root)?;
    let dirstate = repo_root.join(ident.dot_dir()).join("dirstate");
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

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use telemetry::test_utils::generate_fixture;
    use telemetry::test_utils::Fixture;

    use super::*;

    fn repo_path_buf(path: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(path.to_string()).unwrap()
    }

    fn extract_output(io: IO) -> (String, String) {
        let stdout = io.with_output(|o| o.as_any().downcast_ref::<Vec<u8>>().unwrap().clone());
        let stdout = str::from_utf8(&stdout).unwrap().to_string();
        let stderr = io.with_error(|e| {
            e.as_ref()
                .unwrap()
                .as_any()
                .downcast_ref::<Vec<u8>>()
                .unwrap()
                .clone()
        });
        let stderr = str::from_utf8(&stderr).unwrap().to_string();
        (stdout, stderr)
    }

    #[derive(Default)]
    struct GroupStatusTestCase {
        p1: [u8; 20],
        p2: [u8; 20],
        entries: BTreeMap<Vec<u8>, ScmFileStatus>,
        errors: BTreeMap<Vec<u8>, String>,
        dirstate_data_tuples: HashMap<RepoPathBuf, DirstateDataTuple>,
        expected: status::Status,
        stdout: String,
        stderr: String,
    }

    /// Helper function for testing `group_entries`.
    fn test_grouping(test_case: GroupStatusTestCase) {
        let repo_root = generate_fixture(vec![]);
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

        let tin = "".as_bytes();
        let tout = Vec::new();
        let terr = Vec::new();
        let io = IO::new(tin, tout, Some(terr));
        let actual_status = group_entries(repo_root.path(), &status, &dirstate_data, &io).unwrap();
        let (actual_output, actual_error) = extract_output(io);
        assert_eq!(actual_output, test_case.stdout);
        assert_eq!(actual_error, test_case.stderr);
        assert!(actual_status == test_case.expected);
    }

    #[test]
    fn empty_status() {
        test_grouping(Default::default());
    }

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

        let expected = status::StatusBuilder::new()
            .modified(vec![repo_path_buf("modified.txt")])
            .added(vec![
                repo_path_buf("added.txt"),
                repo_path_buf("added_even_though_normally_ignored.txt"),
                repo_path_buf("added_other_parent.txt"),
            ])
            .removed(vec![
                repo_path_buf("modified_and_marked_for_removal.txt"),
                repo_path_buf("removed.txt"),
            ])
            .deleted(vec![repo_path_buf(
                "removed_but_not_marked_for_removal.txt",
            )])
            .unknown(vec![repo_path_buf("unknown.txt")])
            .ignored(vec![repo_path_buf("ignored.txt")])
            .build();

        test_grouping(GroupStatusTestCase {
            entries,
            dirstate_data_tuples,
            expected,
            ..Default::default()
        });
    }

    #[test]
    fn do_not_use_morestatus_if_p2_is_unset() {
        let files = vec![(".hg", Fixture::Dir)];
        let repo_root = generate_fixture(files);
        assert!(!needs_morestatus_extension(
            &repo_root.path().join(".hg"),
            1
        ));
    }

    #[test]
    fn use_morestatus_if_p2_is_set() {
        let files = vec![(".hg", Fixture::Dir)];
        let repo_root = generate_fixture(files);
        assert!(needs_morestatus_extension(&repo_root.path().join(".hg"), 2));
    }

    #[test]
    fn use_morestatus_if_histedit_file_exists() {
        let files = vec![
            (".hg", Fixture::Dir),
            (".hg/histedit-state", Fixture::File(b"")),
        ];
        let repo_root = generate_fixture(files);
        assert!(needs_morestatus_extension(&repo_root.path().join(".hg"), 1));
    }

    #[test]
    fn use_morestatus_if_merge_slash_state_file_exists() {
        let files = vec![
            (".hg", Fixture::Dir),
            (".hg/merge", Fixture::Dir),
            (".hg/merge/state", Fixture::File(b"")),
        ];
        let repo_root = generate_fixture(files);
        assert!(needs_morestatus_extension(&repo_root.path().join(".hg"), 1));
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

        let stdout = "";
        let stderr = format!(
            "{}\n  src/lib: {}\n",
            "Encountered errors computing status for some paths:",
            "unable to fetch directory data: connection reset",
        );

        let status = ScmStatus {
            entries,
            errors,
            ..Default::default()
        };
        let tin = "".as_bytes();
        let tout = Vec::new();
        let terr = Vec::new();
        let io = IO::new(tin, tout, Some(terr));
        let return_code = print_errors(&status, &io).unwrap();
        let (actual_output, actual_error) = extract_output(io);
        assert_eq!(actual_output, stdout);
        assert_eq!(actual_error, stderr);
        assert_eq!(return_code, 1);
    }

    #[test]
    fn status_with_invalid_utf8() {
        let mut entries = BTreeMap::new();
        entries.insert(b"\xb0Z\xd0J\x91\x7f.INFO".to_vec(), ScmFileStatus::ADDED);
        entries.insert(b"modified.txt".to_vec(), ScmFileStatus::MODIFIED);
        let errors = BTreeMap::new();
        let stdout = "";
        let stderr = "skipping invalid utf-8 filename: �Z�J�\u{7f}.INFO (Failed to parse to Utf8: \"�Z�J�\\u{7f}.INFO\". invalid utf-8 sequence of 1 bytes from index 0)\n";
        let expected = status::StatusBuilder::new()
            .modified(vec![repo_path_buf("modified.txt")])
            .build();
        test_grouping(GroupStatusTestCase {
            entries,
            errors,
            expected,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            ..Default::default()
        });
    }
}
