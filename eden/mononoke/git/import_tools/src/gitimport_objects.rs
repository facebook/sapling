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
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::DerivableType;
use encoding_rs::Encoding;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use gix_hash::ObjectId;
use gix_object::bstr::BString;
use gix_object::tree;
use gix_object::Commit;
use gix_object::Tag;
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
use mononoke_types::SortedVectorTrieMap;
use slog::debug;
use slog::Logger;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;

use crate::git_reader::GitReader;
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

#[async_trait]
impl<const SUBMODULES: bool, Store: Send + Sync> Manifest<Store> for GitManifest<SUBMODULES> {
    type TreeId = GitTree<SUBMODULES>;
    type Leaf = (FileType, GitLeaf);
    type TrieMapType = SortedVectorTrieMap<Entry<GitTree<SUBMODULES>, (FileType, GitLeaf)>>;

    async fn lookup(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self.0.get(name).cloned())
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        Ok(stream::iter(self.0.clone().into_iter()).map(Ok).boxed())
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        let entries = self
            .0
            .iter()
            .map(|(k, v)| (k.clone().to_smallvec(), v.clone()))
            .collect();
        Ok(SortedVectorTrieMap::new(entries))
    }
}

async fn load_git_tree<const SUBMODULES: bool, Reader: GitReader>(
    oid: &gix_hash::oid,
    reader: &Reader,
) -> Result<GitManifest<SUBMODULES>, Error> {
    let tree = reader.read_tree(oid).await?;

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

                let r = match mode.into() {
                    tree::EntryKind::Blob => {
                        Some((name, Entry::Leaf((FileType::Regular, GitLeaf(oid)))))
                    }
                    tree::EntryKind::BlobExecutable => {
                        Some((name, Entry::Leaf((FileType::Executable, GitLeaf(oid)))))
                    }
                    tree::EntryKind::Link => {
                        Some((name, Entry::Leaf((FileType::Symlink, GitLeaf(oid)))))
                    }
                    tree::EntryKind::Tree => Some((name, Entry::Tree(GitTree(oid)))),

                    // Git submodules are represented as ObjectType::Commit inside the tree.
                    //
                    // Depending on the repository configuration, we may or may not wish to
                    // include submodules in the imported manifest.  Generate a leaf on the
                    // basis of the SUBMODULES parameter.
                    tree::EntryKind::Commit => {
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
impl<const SUBMODULES: bool, Reader> StoreLoadable<Reader> for GitTree<SUBMODULES>
where
    Reader: GitReader,
{
    type Value = GitManifest<SUBMODULES>;

    async fn load<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        reader: &'a Reader,
    ) -> Result<Self::Value, LoadableError> {
        load_git_tree::<SUBMODULES, Reader>(&self.0, reader)
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
    pub backfill_derivation: BackfillDerivation,
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
            backfill_derivation: BackfillDerivation::No,
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

    /// Returns a Vec of commit hashes to import, ordered so that all
    /// of a commit's parents are listed first
    pub(crate) async fn list_commits(
        &self,
        git_command_path: &Path,
        repo_path: &Path,
    ) -> Result<Vec<Result<ObjectId, Error>>, Error> {
        let mut rev_list = self
            .build_rev_list(git_command_path, repo_path)
            .arg("--topo-order")
            .arg("--reverse")
            .spawn()?;

        self.write_filter_list(&mut rev_list).await?;

        let stdout = BufReader::new(rev_list.stdout.take().context("stdout not set up")?);
        let mut lines = stdout.lines();

        let mut vec = Vec::new();
        while let Some(line) = lines.next_line().await? {
            vec.push(
                line.replace('-', "")
                    .parse()
                    .context("Reading from git rev-list"),
            );
        }
        Ok(vec)
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
    pub target_is_tag: bool,
}

impl TagMetadata {
    pub async fn new<Reader: GitReader>(
        ctx: &CoreContext,
        oid: ObjectId,
        maybe_tag_name: Option<String>,
        reader: &Reader,
    ) -> Result<Self, Error> {
        let Tag {
            name,
            target_kind,
            mut tagger,
            message,
            mut pgp_signature,
            ..
        } = reader.read_tag(&oid).await?;

        let author_date = tagger
            .take()
            .map(|tagger| DateTime::from_gix(tagger.time))
            .transpose()?;
        let author = tagger
            .take()
            .map(|tagger| format_signature(tagger.to_ref()));
        let message = decode_message(&message, &None, ctx.logger())?;
        let name = match maybe_tag_name {
            Some(name) => name,
            None => decode_message(&name, &None, ctx.logger())?,
        };
        let pgp_signature = pgp_signature
            .take()
            .map(|signature| Bytes::from(signature.to_vec()));
        let target_is_tag = target_kind == gix_object::Kind::Tag;
        Result::<_, Error>::Ok(TagMetadata {
            oid,
            author,
            author_date,
            name,
            message,
            pgp_signature,
            target_is_tag,
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

fn format_signature(sig: gix_actor::SignatureRef) -> String {
    format!("{} <{}>", sig.name, sig.email)
}

pub fn decode_with_bom<'a>(
    encoding: &'static Encoding,
    bytes: &'a [u8],
) -> (std::borrow::Cow<'a, str>, &'static Encoding, bool) {
    // Sniff the BOM to see if it overrides the encoding we think we should use
    let encoding = match Encoding::for_bom(bytes) {
        Some((encoding, _bom_length)) => encoding,
        None => encoding,
    };
    // If the encoding is UTF_8, we need to keep the BOM as valid UTF_8 should be
    // round-trippable
    let (cow, had_errors) = encoding.decode_without_bom_handling(bytes);
    (cow, encoding, had_errors)
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
    let explicit_encoding_provided = encoding.is_some();
    let mut encoding_or_utf8 = encoding.clone().unwrap_or_else(|| BString::from("utf-8"));
    // remove single quotes so that "'utf8'" will be accepted
    encoding_or_utf8.retain(|c| *c != 39);

    let encoding = Encoding::for_label(&encoding_or_utf8).ok_or_else(|| {
        anyhow!(
            "Failed to parse git commit encoding: {encoding:?} {}",
            String::from_utf8_lossy(&encoding_or_utf8)
        )
    })?;
    let (decoded, actual_encoding, replacement) = decode_with_bom(encoding, message);
    let message = decoded.to_string();
    if explicit_encoding_provided && actual_encoding != encoding {
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
    pub async fn new<Reader: GitReader>(
        ctx: &CoreContext,
        oid: ObjectId,
        reader: &Reader,
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
        } = reader.read_commit(&oid).await?;

        let tree_oid = tree;
        let parent_tree_oids = {
            let mut trees = HashSet::new();
            for parent in &parents {
                let commit = reader.read_commit(parent).await?;
                trees.insert(commit.tree);
            }
            trees
        };

        let author_date = DateTime::from_gix(author.time)?;
        let committer_date = DateTime::from_gix(committer.time)?;
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
        let original_commit = reader.read_raw_object(&oid).await?;
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
    fn diff_for_submodules<const SUBMODULES: bool, Reader: GitReader>(
        &self,
        ctx: &CoreContext,
        reader: &Reader,
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
    pub fn diff<Reader: GitReader>(
        &self,
        ctx: &CoreContext,
        reader: &Reader,
        submodules: bool,
    ) -> impl Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>> {
        if submodules {
            self.diff_for_submodules::<true, Reader>(ctx, reader)
                .left_stream()
        } else {
            self.diff_for_submodules::<false, Reader>(ctx, reader)
                .right_stream()
        }
    }

    /// Compare the tree for the commit against its parents and return all the trees and subtrees
    /// that have changed w.r.t its parents
    pub fn changed_trees<Reader: GitReader>(
        &self,
        ctx: &CoreContext,
        reader: &Reader,
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

#[derive(Clone, Debug)]
pub enum BackfillDerivation {
    AllConfiguredTypes,
    OnlySpecificTypes(Vec<DerivableType>),
    No,
}

impl BackfillDerivation {
    pub fn types(&self, configured_types: &HashSet<DerivableType>) -> Vec<DerivableType> {
        match self {
            BackfillDerivation::AllConfiguredTypes => configured_types.iter().cloned().collect(),
            BackfillDerivation::OnlySpecificTypes(derived_data_types) => derived_data_types
                .iter()
                .filter(|ty| configured_types.contains(ty))
                .cloned()
                .collect(),
            BackfillDerivation::No => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;
    use slog::o;

    use super::decode_message;
    use super::BString;
    use super::Logger;

    fn should_decode_into(message: &[u8], encoding: &Option<BString>, expected: &str) {
        let logger = Logger::root(slog::Discard, o!());
        let m = decode_message(message, encoding, &logger);
        if m.is_err() {
            panic!("{:?}", m);
        }
        assert_eq!(expected, &m.unwrap())
    }
    fn should_fail_to_decode(message: &[u8], encoding: &Option<BString>) {
        let logger = Logger::root(slog::Discard, o!());
        let m = decode_message(message, encoding, &logger);
        assert!(m.is_err());
    }

    #[mononoke::test]
    fn test_decode_commit_message_given_invalid_encoding_should_fail() {
        should_fail_to_decode(
            b"Hello, World!",
            &Some(BString::from("not a valid encoding label")),
        );
    }
    #[mononoke::test]
    fn test_decode_commit_message_given_ascii_as_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(b"Hello, World!", &encoding, "Hello, World!");
        }
    }
    #[mononoke::test]
    fn test_decode_commit_message_given_valid_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(
                b"Hello, \xce\xba\xe1\xbd\xb9\xcf\x83\xce\xbc\xce\xB5!",
                &encoding,
                "Hello, κόσμε!",
            );
            should_decode_into(
                b"Hello, R\xc3\xa9mi-\xc3\x89tienne!", // UTF-8 encoded
                &encoding,                             // UTF-8 encoding
                "Hello, Rémi-Étienne!",                // Legibly decoded
            );
        }
    }
    #[mononoke::test]
    fn test_decode_commit_message_given_malformed_utf8() {
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(
                b"Hello, R\xe9mi-\xc9tienne!", // Latin 1 encoded
                &encoding,                     // UTF-8 encoding
                "Hello, R�mi-�tienne!", // We have to use replacement characters to encode this
                                        // latin1 string in UTF-8
            );
        }
    }
    #[mononoke::test]
    fn test_decode_commit_message_given_valid_latin1() {
        should_decode_into(
            b"Hello, R\xe9mi-\xc9tienne!",      // Latin 1 encoded
            &Some(BString::from("iso-8859-1")), // Latin 1 encoding
            "Hello, Rémi-Étienne!",             // We decode just fine into legible UTF-8
        );
    }
    #[mononoke::test]
    fn test_decode_commit_message_given_malformed_latin1() {
        should_decode_into(
            b"Hello, R\xc3\xa9mi-\xc3\x89tienne!".as_slice(), // UTF-8 encoded
            &Some(BString::from("iso-8859-1")),               // Latin 1 encoding
            "Hello, RÃ©mi-Ã‰tienne!", // Broken decoding, this is the best we can do
        );
    }
    #[mononoke::test]
    fn test_decode_utf8_with_bom() {
        // We can sniff the UTF-8 BOM mark
        assert_eq!(
            encoding_rs::Encoding::for_bom(b"\xef\xbb\xbf"),
            Some((encoding_rs::UTF_8, 3))
        );
        for encoding in [None, Some(BString::from("utf-8"))] {
            should_decode_into(
                b"\xef\xbb\xbfHello, World!",
                &encoding,
                "\u{feff}Hello, World!",
            );
        }
    }
    #[mononoke::test]
    fn test_decode_non_utf8_with_bom() {
        // We can sniff the UTF-16BE BOM mark
        assert_eq!(
            encoding_rs::Encoding::for_bom(b"\xfe\xff"),
            Some((encoding_rs::UTF_16BE, 2))
        );
        for encoding in [None, Some(BString::from("utf-16be"))] {
            should_decode_into(
                b"\xfe\xff\xd8\x34\xdd\x1e\x00 \x00H\x00i\x00!",
                &encoding,
                "\u{feff}𝄞 Hi!",
            );
        }
        // Mismatch between the encoding in the BOM and the declared encoding
        should_fail_to_decode(
            b"\xfe\xff\xd8\x34\xdd\x1e\x00 \x00H\x00i\x00!",
            &Some(BString::from("utf-8")),
        );
    }
    #[mononoke::test]
    fn test_decode_gb18030_with_bom_shows_the_limits_of_our_implementation() {
        // b"\x84\x31\x95\x33" is the BOM mark that indicates a GB18030 encoding. An encoding for
        // chinese characters.
        // Currently, BOM-sniffing doesn't work for this esoteric encoding due to limitations of
        // encoding_rs.
        // This means we would need for the encoding to be explicitly specified to be able to
        // decode such strings without falling back to replacement characters
        assert_eq!(encoding_rs::Encoding::for_bom(b"\x84\x31\x95\x33"), None);
        should_decode_into(b"\x84\x31\x95\x33Hello, \xfe\x55!", &None, "�1�3Hello, �U!");
        // Explicit gb18030 encoding
        should_decode_into(
            b"\x84\x31\x95\x33Hello, \xfe\x55!",
            &Some(BString::from("gb18030")),
            "\u{feff}Hello, 㑳!",
        );
        should_decode_into(
            b"\x84\x31\x95\x33Hello, \xfe\x55!", // GB18030 encoded
            &Some(BString::from("utf-8")),       // UTF-8 encoding
            "�1�3Hello, �U!",                    // We have to use replacement characters
        );
    }
}
