/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use encoding_rs::Encoding;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use git_hash::ObjectId;
use git_object::bstr::BString;
use git_object::tree;
use git_object::Commit;
use git_object::Tree;
use manifest::Entry;
use manifest::Manifest;
use manifest::StoreLoadable;
use mononoke_types::hash;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use slog::debug;
use slog::Logger;
use sorted_vector_map::SortedVectorMap;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio_stream::wrappers::LinesStream;

use crate::git_reader::GitRepoReader;
use crate::gitlfs::GitImportLfs;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitTree(pub ObjectId);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitLeaf(pub ObjectId);

pub struct GitManifest(HashMap<MPathElement, Entry<GitTree, (FileType, GitLeaf)>>);

impl Manifest for GitManifest {
    type TreeId = GitTree;
    type LeafId = (FileType, GitLeaf);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(self.0.clone().into_iter())
    }
}

async fn read_tree(reader: &GitRepoReader, oid: &git_hash::oid) -> Result<Tree, Error> {
    let object = reader.get_object(oid).await?;
    object
        .try_into_tree()
        .map_err(|_| format_err!("{} is not a tree", oid))
}

async fn load_git_tree(oid: &git_hash::oid, reader: &GitRepoReader) -> Result<GitManifest, Error> {
    let tree = read_tree(reader, oid).await?;

    let elements = tree
        .entries
        .into_iter()
        .filter_map(
            |tree::Entry {
                 mode,
                 filename,
                 oid,
             }| {
                let name = match MPathElement::new(filename.into()) {
                    Ok(name) => name,
                    Err(e) => return Some(Err(e)),
                };

                let r = match mode {
                    tree::EntryMode::Blob => {
                        Some((name, Entry::Leaf((FileType::Regular, GitLeaf(oid)))))
                    }
                    tree::EntryMode::BlobExecutable => {
                        Some((name, Entry::Leaf((FileType::Executable, GitLeaf(oid)))))
                    }
                    tree::EntryMode::Link => {
                        Some((name, Entry::Leaf((FileType::Symlink, GitLeaf(oid)))))
                    }
                    tree::EntryMode::Tree => Some((name, Entry::Tree(GitTree(oid)))),

                    // git-sub-modules are represented as ObjectType::Commit inside the tree.
                    // For now we do not support git-sub-modules but we still need to import
                    // repositories that has sub-modules in them (just not synchronized), so
                    // ignoring any sub-module for now.
                    tree::EntryMode::Commit => None,
                };
                anyhow::Ok(r).transpose()
            },
        )
        .collect::<Result<_, Error>>()?;

    anyhow::Ok(GitManifest(elements))
}

#[async_trait]
impl StoreLoadable<GitRepoReader> for GitTree {
    type Value = GitManifest;

    async fn load<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        reader: &'a GitRepoReader,
    ) -> Result<Self::Value, LoadableError> {
        load_git_tree(&self.0, reader)
            .await
            .map_err(LoadableError::from)
    }
}

#[derive(Clone, Debug)]
pub struct GitimportPreferences {
    pub dry_run: bool,
    /// Only for logging purpuses,
    /// useful when several repos are imported simultainously.
    pub gitrepo_name: Option<String>,
    pub concurrency: usize,
    pub lfs: GitImportLfs,
    pub git_command_path: PathBuf,
}

impl Default for GitimportPreferences {
    fn default() -> Self {
        GitimportPreferences {
            dry_run: false,
            gitrepo_name: None,
            concurrency: 20,
            lfs: GitImportLfs::default(),
            git_command_path: PathBuf::from("/usr/bin/git.real"),
        }
    }
}

pub fn oid_to_sha1(oid: &git_hash::oid) -> Result<hash::GitSha1, Error> {
    hash::GitSha1::from_bytes(oid.as_bytes())
}

/// Determines which commits to import
pub struct GitimportTarget {
    // If both are empty, we'll grab all commits
    wanted: Vec<ObjectId>,
    known: HashMap<ObjectId, ChangesetId>,
}

impl GitimportTarget {
    /// Import the full repo
    pub fn full() -> Self {
        Self {
            wanted: Vec::new(),
            known: HashMap::new(),
        }
    }

    pub fn new(wanted: ObjectId, known: HashMap<ObjectId, ChangesetId>) -> Result<Self, Error> {
        Ok(Self {
            wanted: vec![wanted],
            known,
        })
    }

    /// Roots are the Oid -> ChangesetId mappings that already are
    /// imported into Mononoke.
    pub fn get_roots(&self) -> &HashMap<ObjectId, ChangesetId> {
        &self.known
    }

    /// Returns the number of commits to import
    pub async fn get_nb_commits(
        &self,
        git_command_path: &Path,
        repo_path: &Path,
    ) -> Result<usize, Error> {
        let mut rev_list = self
            .build_rev_list(git_command_path, repo_path)
            .arg("--count")
            .spawn()?;

        self.write_filter_list(&mut rev_list).await?;

        // stdout is a single line that parses as number of commits
        let stdout = BufReader::new(rev_list.stdout.take().context("stdout not set up")?);
        let mut lines = stdout.lines();
        if let Some(line) = lines.next_line().await? {
            Ok(line.parse()?)
        } else {
            bail!("No lines returned by git rev-list");
        }
    }

    /// Returns a stream of commit hashes to import, ordered so that all
    /// of a commit's parents are listed first
    pub(crate) async fn list_commits(
        &self,
        git_command_path: &Path,
        repo_path: &Path,
    ) -> Result<impl Stream<Item = Result<ObjectId, Error>>, Error> {
        let mut rev_list = self
            .build_rev_list(git_command_path, repo_path)
            .arg("--topo-order")
            .arg("--reverse")
            .spawn()?;

        self.write_filter_list(&mut rev_list).await?;

        let stdout = BufReader::new(rev_list.stdout.take().context("stdout not set up")?);
        let lines_stream = LinesStream::new(stdout.lines());

        Ok(lines_stream
            .err_into()
            .and_then(|line| async move { line.parse().context("Reading from git rev-list") }))
    }

    async fn write_filter_list(&self, rev_list: &mut Child) -> Result<(), Error> {
        if !self.wanted.is_empty() {
            let mut stdin = rev_list.stdin.take().context("stdin not set up properly")?;
            for commit in &self.wanted {
                stdin.write_all(format!("{}\n", commit).as_bytes()).await?;
            }
            for commit in self.known.keys() {
                stdin.write_all(format!("^{}\n", commit).as_bytes()).await?;
            }
        }

        Ok(())
    }

    fn build_rev_list(&self, git_command_path: &Path, repo_path: &Path) -> Command {
        let mut command = Command::new(git_command_path);
        command
            .current_dir(repo_path)
            .env_clear()
            .kill_on_drop(false)
            .stdout(Stdio::piped())
            .arg("rev-list");

        if self.wanted.is_empty() {
            command.arg("--all").stdin(Stdio::null());
        } else {
            command.arg("--stdin").stdin(Stdio::piped());
        }
        command
    }
}

pub struct CommitMetadata {
    pub oid: ObjectId,
    pub parents: Vec<ObjectId>,
    pub message: String,
    pub author: String,
    pub author_date: DateTime,
    pub committer: String,
    pub committer_date: DateTime,
}

pub struct ExtractedCommit {
    pub metadata: CommitMetadata,
    pub tree: GitTree,
    pub parent_trees: HashSet<GitTree>,
}

pub(crate) async fn read_commit(
    reader: &GitRepoReader,
    oid: &git_hash::oid,
) -> Result<Commit, Error> {
    let object = reader.get_object(oid).await?;
    object
        .try_into_commit()
        .map_err(|_| format_err!("{} is not a commit", oid))
}

fn format_signature(sig: git_actor::SignatureRef) -> String {
    format!("{} <{}>", sig.name, sig.email)
}

/// Decode a git commit message
///
/// Git choses to keep the raw user-provided bytes for the commit message.
/// That is to avoid a possibly lossy conversion to UTF-8.
/// Git provides an option to set the encoding by setting i18n.commitEncoding in .git/config.
/// See [the git documentation](https://git-scm.com/docs/git-commit#_discussion) for a discussion
/// of that design choice.
///
/// In contrast, mononoke stores commit messages in UTF-8.
///
/// This means that importing a git commit message can be lossy. For instance, if a git user used a
/// non-UTF-8 compatible encoding such as latin1, but didn't set the `commitEncoding` setting
/// accordingly, the conversion will be lossy.
/// These latin1-encoded bytes: `b"Hello, R\xe9mi-\xc9tienne!"` will convert to `"Hello, R�mi-�tienne!"`
/// if the encoding is not specified (so it will default to UTF-8).
fn decode_commit_message(
    message: &[u8],
    encoding: &Option<BString>,
    logger: &Logger,
) -> Result<String, Error> {
    let encoding = Encoding::for_label(&encoding.clone().unwrap_or_else(|| BString::from("utf-8")))
        .ok_or_else(|| anyhow!("Failed to parse git commit encoding: {encoding:?}"))?;
    let (decoded, actual_encoding, replacement) = encoding.decode(message);
    let message = decoded.to_string();
    if actual_encoding != encoding {
        // Decode performs BOM sniffing to detect the actual encoding for this byte string.
        // We expect it to match the encoding declared in the commit metadata.
        bail!("Unexpected encoding: expected {encoding:?}, got {actual_encoding:?}");
    } else if replacement {
        // If the input string contains malformed sequences, they get replaced with the
        // REPLACEMENT CHARACTER.
        // In this situation, don't fail but log the occurrence.
        debug!(
            logger,
            "Failed to decode git commit message:\n{message:?}\nwith encoding: {encoding:?}.\nThe offending characters were replaced"
        );
    }
    Ok(message)
}

impl ExtractedCommit {
    pub async fn new(
        ctx: &CoreContext,
        oid: ObjectId,
        reader: &GitRepoReader,
    ) -> Result<Self, Error> {
        let Commit {
            tree,
            parents,
            author,
            committer,
            encoding,
            message,
            ..
        } = read_commit(reader, &oid).await?;

        let tree = GitTree(tree);

        let parent_trees = {
            let mut trees = HashSet::new();
            for parent in &parents {
                let commit = read_commit(reader, parent).await?;
                trees.insert(GitTree(commit.tree));
            }
            trees
        };

        let author_date = convert_time_to_datetime(&author.time)?;
        let committer_date = convert_time_to_datetime(&committer.time)?;
        let author = format_signature(author.to_ref());
        let committer = format_signature(committer.to_ref());
        let message = decode_commit_message(&message, &encoding, ctx.logger())?;
        let parents = parents.into_vec();

        Result::<_, Error>::Ok(ExtractedCommit {
            metadata: CommitMetadata {
                oid,
                parents,
                message,
                author,
                author_date,
                committer,
                committer_date,
            },
            tree,
            parent_trees,
        })
    }
}

pub fn convert_time_to_datetime(time: &git_actor::Time) -> Result<DateTime, Error> {
    DateTime::from_timestamp(
        time.seconds_since_unix_epoch.into(),
        -time.offset_in_seconds,
    )
}

#[async_trait]
pub trait GitUploader: Clone + Send + Sync + 'static {
    /// The type of a file change to be uploaded
    type Change: Clone + Send + Sync + 'static;

    /// The type of a changeset returned by generate_changeset
    type IntermediateChangeset: Send + Sync;

    /// Returns a change representing a deletion
    fn deleted() -> Self::Change;

    /// Looks to see if we can elide importing a commit
    /// If you can give us the ChangesetId for a given git object,
    /// then we assume that it's already imported and skip it
    async fn check_commit_uploaded(
        &self,
        ctx: &CoreContext,
        oid: &git_hash::oid,
    ) -> Result<Option<ChangesetId>, Error>;

    /// Upload a single file to the repo
    async fn upload_file(
        &self,
        ctx: &CoreContext,
        lfs: &GitImportLfs,
        path: &MPath,
        ty: FileType,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<Self::Change, Error>;

    /// Generate a single Bonsai changeset ID
    /// This should delay saving the changeset if possible
    /// but may save it if required.
    ///
    /// You are guaranteed that all parents of the given changeset
    /// have been generated by this point.
    async fn generate_changeset(
        &self,
        ctx: &CoreContext,
        bonsai_parents: Vec<ChangesetId>,
        metadata: CommitMetadata,
        changes: SortedVectorMap<MPath, Self::Change>,
        dry_run: bool,
    ) -> Result<(Self::IntermediateChangeset, ChangesetId), Error>;

    /// Finalize a batch of generated changesets. The supplied batch is
    /// topologically sorted so that parents are all present before children
    /// If you did not finalize the changeset in generate_changeset,
    /// you must do so here.
    async fn finalize_batch(
        &self,
        ctx: &CoreContext,
        dry_run: bool,
        changesets: Vec<(Self::IntermediateChangeset, hash::GitSha1)>,
    ) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use slog::o;

    use super::decode_commit_message;
    use super::BString;
    use super::Logger;

    const ASCII_BSTR: &[u8] = b"Hello, World!".as_slice();
    const ASCII_STR: &str = "Hello, World!";
    const UTF8_UNICODE_BSTR: &[u8] =
        b"Hello, \xce\xba\xe1\xbd\xb9\xcf\x83\xce\xbc\xce\xB5!".as_slice();
    const UTF8_UNICODE_STR: &str = "Hello, κόσμε!";
    const LATIN1_ACCENTED_BSTR: &[u8] = b"Hello, R\xe9mi-\xc9tienne!".as_slice();
    const UTF8_ACCENTED_BSTR: &[u8] = b"Hello, R\xc3\xa9mi-\xc3\x89tienne!".as_slice();
    const BROKEN_LATIN1_FROM_UTF8_ACCENTED_STR: &str = "Hello, RÃ©mi-Ã‰tienne!";
    const UTF8_ACCENTED_STR: &str = "Hello, Rémi-Étienne!";
    const UTF8_ACCENTED_STR_WITH_REPLACEMENT_CHARACTER: &str = "Hello, R�mi-�tienne!";

    fn should_decode_into(message: &[u8], encoding: &Option<BString>, expected: &str) {
        let logger = Logger::root(slog::Discard, o!());
        let m = decode_commit_message(message, encoding, &logger);
        assert!(m.is_ok());
        assert_eq!(expected, &m.unwrap())
    }
    fn should_fail_to_decode(message: &[u8], encoding: &Option<BString>) {
        let logger = Logger::root(slog::Discard, o!());
        let m = decode_commit_message(message, encoding, &logger);
        assert!(m.is_err());
    }

    #[test]
    fn test_decode_commit_message_given_invalid_encoding_should_fail() {
        should_fail_to_decode(
            ASCII_BSTR,
            &Some(BString::from("not a valid encoding label")),
        );
    }
    #[test]
    fn test_decode_commit_message_given_ascii_as_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(ASCII_BSTR, &encoding, ASCII_STR);
        }
    }
    #[test]
    fn test_decode_commit_message_given_valid_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(UTF8_UNICODE_BSTR, &encoding, UTF8_UNICODE_STR);
            should_decode_into(UTF8_ACCENTED_BSTR, &encoding, UTF8_ACCENTED_STR);
        }
    }
    #[test]
    fn test_decode_commit_message_given_malformed_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(
                LATIN1_ACCENTED_BSTR,
                &encoding,
                UTF8_ACCENTED_STR_WITH_REPLACEMENT_CHARACTER,
            );
        }
    }
    #[test]
    fn test_decode_commit_message_given_valid_latin1() {
        should_decode_into(
            LATIN1_ACCENTED_BSTR,
            &Some(BString::from("iso-8859-1")),
            UTF8_ACCENTED_STR,
        );
    }
    #[test]
    fn test_decode_commit_message_given_malformed_latin1() {
        should_decode_into(
            UTF8_ACCENTED_BSTR,
            &Some(BString::from("iso-8859-1")),
            BROKEN_LATIN1_FROM_UTF8_ACCENTED_STR,
        );
    }
}
