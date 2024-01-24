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
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use gix_hash::ObjectId;
use gix_object::bstr::BString;
use gix_object::tree;
use gix_object::Commit;
use gix_object::Tag;
use gix_object::Tree;
use manifest::bonsai_diff;
use manifest::find_intersection_of_diffs;
use manifest::BonsaiDiffFileChange;
use manifest::Entry;
use manifest::Manifest;
use manifest::StoreLoadable;
use mononoke_types::hash;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileType;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use slog::debug;
use slog::Logger;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio_stream::wrappers::LinesStream;

use crate::git_reader::GitRepoReader;
use crate::gitlfs::GitImportLfs;

/// An imported git tree object reference.
///
/// If SUBMODULES is true, submodules in this tree and its descendants are included.
///
/// If SUBMODULES is false, submodules in this tree and its descendants are dropped.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitTree<const SUBMODULES: bool>(pub ObjectId);

/// An imported git leaf object reference (blob or submodule).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitLeaf(pub ObjectId);

/// An imported git tree in manifest form.
///
/// If SUBMODULES is true, submodules in this tree and its descendants are included.
///
/// If SUBMODULES is false, submodules in this tree and its descendants are dropped.
pub struct GitManifest<const SUBMODULES: bool>(
    HashMap<MPathElement, Entry<GitTree<SUBMODULES>, (FileType, GitLeaf)>>,
);

impl<const SUBMODULES: bool> Manifest for GitManifest<SUBMODULES> {
    type TreeId = GitTree<SUBMODULES>;
    type LeafId = (FileType, GitLeaf);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(self.0.clone().into_iter())
    }
}

async fn read_tree(reader: &GitRepoReader, oid: &gix_hash::oid) -> Result<Tree, Error> {
    let object = reader.get_object(oid).await?;
    object
        .parsed
        .try_into_tree()
        .map_err(|_| format_err!("{} is not a tree", oid))
}

async fn load_git_tree<const SUBMODULES: bool>(
    oid: &gix_hash::oid,
    reader: &GitRepoReader,
) -> Result<GitManifest<SUBMODULES>, Error> {
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

                    // Git submodules are represented as ObjectType::Commit inside the tree.
                    //
                    // Depending on the repository configuration, we may or may not wish to
                    // include submodules in the imported manifest.  Generate a leaf on the
                    // basis of the SUBMODULES parameter.
                    tree::EntryMode::Commit => {
                        if SUBMODULES {
                            Some((name, Entry::Leaf((FileType::GitSubmodule, GitLeaf(oid)))))
                        } else {
                            None
                        }
                    }
                };
                anyhow::Ok(r).transpose()
            },
        )
        .collect::<Result<_, Error>>()?;

    anyhow::Ok(GitManifest(elements))
}

#[async_trait]
impl<const SUBMODULES: bool> StoreLoadable<GitRepoReader> for GitTree<SUBMODULES> {
    type Value = GitManifest<SUBMODULES>;

    async fn load<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        reader: &'a GitRepoReader,
    ) -> Result<Self::Value, LoadableError> {
        load_git_tree::<SUBMODULES>(&self.0, reader)
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
    /// Whether submodules should be imported instead of dropped.
    pub submodules: bool,
    pub lfs: GitImportLfs,
    pub git_command_path: PathBuf,
}

impl Default for GitimportPreferences {
    fn default() -> Self {
        GitimportPreferences {
            dry_run: false,
            gitrepo_name: None,
            concurrency: 20,
            submodules: true,
            lfs: GitImportLfs::default(),
            git_command_path: PathBuf::from("/usr/bin/git.real"),
        }
    }
}

pub fn oid_to_sha1(oid: &gix_hash::oid) -> Result<hash::GitSha1, Error> {
    hash::GitSha1::from_bytes(oid.as_bytes())
}

/// Determines which commits to import
pub struct GitimportTarget {
    // If both are empty, we'll grab all commits
    // TODO: The None case is only used by Mononoke - see if we can get the Mononoke team
    // to let us remove this and just store the ObjectId directly
    wanted: Option<ObjectId>,
    known: HashMap<ObjectId, ChangesetId>,
}

impl GitimportTarget {
    /// Import the full repo
    pub fn full() -> Self {
        Self {
            wanted: None,
            known: HashMap::new(),
        }
    }

    pub fn new(wanted: ObjectId, known: HashMap<ObjectId, ChangesetId>) -> Result<Self, Error> {
        Ok(Self {
            wanted: Some(wanted),
            known,
        })
    }

    /// Roots are the Oid -> ChangesetId mappings that already are
    /// imported into Mononoke.
    pub fn get_roots(&self) -> &HashMap<ObjectId, ChangesetId> {
        &self.known
    }

    /// Returns true if wanted commit is already imported
    pub fn is_already_imported(&self) -> bool {
        if let Some(wanted) = self.wanted.as_ref() {
            self.known.contains_key(wanted)
        } else {
            false
        }
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

        Ok(lines_stream.err_into().and_then(|line| async move {
            // rev-list with --boundary option returns boundary commits prefixed with `-`
            // here we remove that prefix to get uniformed list of commits
            line.replace('-', "")
                .parse()
                .context("Reading from git rev-list")
        }))
    }

    async fn write_filter_list(&self, rev_list: &mut Child) -> Result<(), Error> {
        if let Some(wanted) = self.wanted.as_ref() {
            let mut stdin = rev_list.stdin.take().context("stdin not set up properly")?;
            stdin.write_all(format!("{}\n", wanted).as_bytes()).await?;
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
            .arg("rev-list")
            .arg("--boundary");

        if self.wanted.is_none() {
            command.arg("--all").stdin(Stdio::null());
        } else {
            command.arg("--stdin").stdin(Stdio::piped());
        }
        command
    }
}

#[derive(Debug)]
pub struct TagMetadata {
    pub oid: ObjectId,
    pub message: String,
    pub author: Option<String>,
    pub author_date: Option<DateTime>,
    pub name: String,
    pub pgp_signature: Option<Bytes>,
}

impl TagMetadata {
    pub async fn new(
        ctx: &CoreContext,
        oid: ObjectId,
        reader: &GitRepoReader,
    ) -> Result<Self, Error> {
        let Tag {
            name,
            mut tagger,
            message,
            mut pgp_signature,
            ..
        } = read_tag(reader, &oid).await?;

        let author_date = tagger
            .take()
            .map(|tagger| convert_time_to_datetime(&tagger.time))
            .transpose()?;
        let author = tagger
            .take()
            .map(|tagger| format_signature(tagger.to_ref()));
        let message = decode_message(&message, &None, ctx.logger())?;
        let name = decode_message(&name, &None, ctx.logger())?;
        let pgp_signature = pgp_signature
            .take()
            .map(|signature| Bytes::from(signature.to_vec()));
        Result::<_, Error>::Ok(TagMetadata {
            oid,
            author,
            author_date,
            name,
            message,
            pgp_signature,
        })
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
    pub git_extra_headers: SortedVectorMap<SmallVec<[u8; 24]>, Bytes>,
}

pub struct ExtractedCommit {
    pub metadata: CommitMetadata,
    pub tree_oid: ObjectId,
    pub parent_tree_oids: HashSet<ObjectId>,
    pub original_commit: Bytes,
}

pub(crate) async fn read_tag(reader: &GitRepoReader, oid: &gix_hash::oid) -> Result<Tag, Error> {
    let object = reader.get_object(oid).await?;
    object
        .parsed
        .try_into_tag()
        .map_err(|_| format_err!("{} is not a tag", oid))
}

pub(crate) async fn read_commit(
    reader: &GitRepoReader,
    oid: &gix_hash::oid,
) -> Result<Commit, Error> {
    let object = reader.get_object(oid).await?;
    object
        .parsed
        .try_into_commit()
        .map_err(|_| format_err!("{} is not a commit", oid))
}

pub(crate) async fn read_raw_object(
    reader: &GitRepoReader,
    oid: &gix_hash::oid,
) -> Result<Bytes, Error> {
    reader
        .get_object(oid)
        .await
        .map(|obj| obj.raw)
        .with_context(|| format!("Error while fetching Git object for ID {}", oid))
}

fn format_signature(sig: gix_actor::SignatureRef) -> String {
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
fn decode_message(
    message: &[u8],
    encoding: &Option<BString>,
    logger: &Logger,
) -> Result<String, Error> {
    let mut encoding_or_utf8 = encoding.clone().unwrap_or_else(|| BString::from("utf-8"));
    // remove single quotes so that "'utf8'" will be accepted
    encoding_or_utf8.retain(|c| *c != 39);

    let encoding = Encoding::for_label(&encoding_or_utf8).ok_or_else(|| {
        anyhow!(
            "Failed to parse git commit encoding: {encoding:?} {}",
            String::from_utf8_lossy(&encoding_or_utf8)
        )
    })?;
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
            "Failed to decode git message:\n{message:?}\nwith encoding: {encoding:?}.\nThe offending characters were replaced"
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
            extra_headers,
            ..
        } = read_commit(reader, &oid).await?;

        let tree_oid = tree;
        let parent_tree_oids = {
            let mut trees = HashSet::new();
            for parent in &parents {
                let commit = read_commit(reader, parent).await?;
                trees.insert(commit.tree);
            }
            trees
        };

        let author_date = convert_time_to_datetime(&author.time)?;
        let committer_date = convert_time_to_datetime(&committer.time)?;
        let author = format_signature(author.to_ref());
        let committer = format_signature(committer.to_ref());
        let message = decode_message(&message, &encoding, ctx.logger())?;
        let parents = parents.into_vec();
        let git_extra_headers = extra_headers
            .into_iter()
            .map(|(k, v)| {
                (
                    SmallVec::from(k.as_slice()),
                    Bytes::copy_from_slice(v.as_slice()),
                )
            })
            .collect();
        let original_commit = read_raw_object(reader, &oid).await?;
        Result::<_, Error>::Ok(ExtractedCommit {
            original_commit,
            metadata: CommitMetadata {
                oid,
                parents,
                message,
                author,
                author_date,
                committer,
                committer_date,
                git_extra_headers,
            },
            tree_oid,
            parent_tree_oids,
        })
    }

    /// Generic version of `diff` based on whether submodules are
    /// included or not.
    fn diff_for_submodules<const SUBMODULES: bool>(
        &self,
        ctx: &CoreContext,
        reader: &GitRepoReader,
    ) -> impl Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>> {
        let tree = GitTree::<SUBMODULES>(self.tree_oid);
        let parent_trees = self
            .parent_tree_oids
            .iter()
            .cloned()
            .map(GitTree::<SUBMODULES>)
            .collect();
        bonsai_diff(ctx.clone(), reader.clone(), tree, parent_trees)
    }

    /// Compare the commit against its parents and return all bonsai changes
    /// that it includes.
    pub fn diff(
        &self,
        ctx: &CoreContext,
        reader: &GitRepoReader,
        submodules: bool,
    ) -> impl Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>> {
        if submodules {
            self.diff_for_submodules::<true>(ctx, reader).left_stream()
        } else {
            self.diff_for_submodules::<false>(ctx, reader)
                .right_stream()
        }
    }

    /// Compare the tree for the commit against its parents and return all the trees and subtrees
    /// that have changed w.r.t its parents
    pub fn changed_trees(
        &self,
        ctx: &CoreContext,
        reader: &GitRepoReader,
    ) -> impl Stream<Item = Result<GitTree<true>, Error>> {
        // When doing manifest diff over trees, submodules enabled or disabled doesn't matter
        let tree = GitTree::<true>(self.tree_oid);
        let parent_trees = self
            .parent_tree_oids
            .iter()
            .cloned()
            .map(GitTree::<true>)
            .collect();
        find_intersection_of_diffs(ctx.clone(), reader.clone(), tree, parent_trees)
            .try_filter_map(|(_, entry)| async move {
                let result = match entry {
                    Entry::Tree(git_tree) => Some(git_tree),
                    Entry::Leaf(_) => None,
                };
                anyhow::Ok(result)
            })
            .boxed()
    }

    /// Generic version of `diff_root` based on whether submodules are
    /// included or not.
    fn diff_root_for_submodules<const SUBMODULES: bool>(
        &self,
        ctx: &CoreContext,
        reader: &GitRepoReader,
    ) -> impl Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>> {
        let tree = GitTree::<SUBMODULES>(self.tree_oid);
        bonsai_diff(ctx.clone(), reader.clone(), tree, HashSet::new())
    }

    /// Return all of the bonsai changes that this commit includes, as if it
    /// is a root commit (i.e. compare it against an empty tree).
    pub fn diff_root(
        &self,
        ctx: &CoreContext,
        reader: &GitRepoReader,
        submodules: bool,
    ) -> impl Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>> {
        if submodules {
            self.diff_root_for_submodules::<true>(ctx, reader)
                .left_stream()
        } else {
            self.diff_root_for_submodules::<false>(ctx, reader)
                .right_stream()
        }
    }
}

pub fn convert_time_to_datetime(time: &gix_date::Time) -> Result<DateTime, Error> {
    DateTime::from_timestamp(time.seconds, -time.offset)
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
        oid: &gix_hash::oid,
    ) -> Result<Option<ChangesetId>, Error>;

    /// Upload a single file to the repo
    async fn upload_file(
        &self,
        ctx: &CoreContext,
        lfs: &GitImportLfs,
        path: &NonRootMPath,
        ty: FileType,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<Self::Change, Error>;

    /// Upload a single git object to the repo blobstore of the mercurial mirror.
    /// Use this method for uploading non-blob git objects (e.g. tree, commit, etc)
    async fn upload_object(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error>;

    /// Upload a single packfile item corresponding to a git base object, i.e. commit,
    /// tree, blob or tag
    async fn upload_packfile_base_item(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error>;

    /// Generate a single Bonsai changeset ID for corresponding Git commit
    /// This should delay saving the changeset if possible
    /// but may save it if required.
    ///
    /// You are guaranteed that all parents of the given changeset
    /// have been generated by this point.
    async fn generate_changeset_for_commit(
        &self,
        ctx: &CoreContext,
        bonsai_parents: Vec<ChangesetId>,
        metadata: CommitMetadata,
        changes: SortedVectorMap<NonRootMPath, Self::Change>,
        dry_run: bool,
    ) -> Result<(Self::IntermediateChangeset, ChangesetId), Error>;

    /// Generate a single Bonsai changeset ID for corresponding Git
    /// annotated tag.
    async fn generate_changeset_for_annotated_tag(
        &self,
        ctx: &CoreContext,
        target_changeset_id: ChangesetId,
        tag: TagMetadata,
    ) -> Result<ChangesetId, Error>;

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

    use super::decode_message;
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
        let m = decode_message(message, encoding, &logger);
        assert!(m.is_ok());
        assert_eq!(expected, &m.unwrap())
    }
    fn should_fail_to_decode(message: &[u8], encoding: &Option<BString>) {
        let logger = Logger::root(slog::Discard, o!());
        let m = decode_message(message, encoding, &logger);
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
