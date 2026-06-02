/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use acl_manifest::RootAclManifestId;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_stream::try_stream;
use clap::Args;
use clap::ValueEnum;
use cloned::cloned;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derivation_queue_thrift::DerivationPriority;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivedDataManager;
use directory_branch_cluster_manifest::RootDirectoryBranchClusterManifestId;
use fsnodes::RootFsnodeId;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use git_types::GitLeaf;
use git_types::GitTreeId;
use git_types::MappedGitCommitId;
use history_manifest::RootHistoryManifestDirectoryId;
use manifest::Entry;
use manifest::Manifest;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::StoreLoadable;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::ChangesetId;
use mononoke_types::ContentManifestId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestDirectoryRestriction;
use mononoke_types::acl_manifest::AclManifestEntry;
use mononoke_types::content_manifest::ContentManifestFile;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::deleted_manifest_v2::DeletedManifestV2;
use mononoke_types::directory_branch_cluster_manifest::DirectoryBranchClusterManifest;
use mononoke_types::directory_branch_cluster_manifest::DirectoryBranchClusterManifestFile;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::history_manifest::HistoryManifestDeletedNode;
use mononoke_types::history_manifest::HistoryManifestDirectory;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::history_manifest::HistoryManifestFile;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::typed_hash::AclManifestId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use skeleton_manifest::RootSkeletonManifestId;
use skeleton_manifest_v2::RootSkeletonManifestV2Id;
use unodes::RootUnodeManifestId;

use super::Repo;

/// Supported manifest types
#[derive(Copy, Clone, Debug, ValueEnum)]
enum ListManifestType {
    SkeletonManifests,
    SkeletonManifests2,
    ContentManifests,
    DirectoryBranchClusterManifests,
    Fsnodes,
    Unodes,
    DeletedManifests,
    HgManifests,
    HgAugmentedManifests,
    GitTrees,
    AclManifests,
    HistoryManifests,
}

#[derive(Args)]
pub(super) struct ListManifestArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    /// Path you want to examine
    #[clap(long, short)]
    path: Option<String>,
    /// List the directory itself, not its contents
    #[clap(long, short)]
    directory: bool,
    /// List recursively
    #[clap(long, short, conflicts_with = "directory")]
    recursive: bool,
    /// Derive the manifest if it hasn't already been derived
    #[clap(long)]
    derive: bool,
    /// Type of manifest to list
    #[clap(long, short = 't', value_enum)]
    manifest_type: ListManifestType,
}

enum ListItem {
    Directory(MPath, String),
    File(MPath, String),
}

impl ListItem {
    fn new<TreeId, Leaf>(path: MPath, entry: Entry<TreeId, Leaf>) -> Self
    where
        Entry<TreeId, Leaf>: Listable,
    {
        match entry {
            Entry::Tree(..) => ListItem::Directory(path, entry.list_item()),
            Entry::Leaf(..) => ListItem::File(path, entry.list_item()),
        }
    }

    fn new_history_manifest(path: MPath, entry: &HistoryManifestEntry, desc: String) -> Self {
        match entry {
            HistoryManifestEntry::File(_) => ListItem::File(path, desc),
            HistoryManifestEntry::Directory(_) => ListItem::Directory(path, desc),
            // DeletedNode could be either a file or directory — treat as file for listing.
            HistoryManifestEntry::DeletedNode(_) => ListItem::File(path, desc),
        }
    }

    fn new_deleted(path: MPath, entry_id: DeletedManifestV2Id, entry: DeletedManifestV2) -> Self {
        let desc = if let Some(linknode) = entry.linknode() {
            format!("{entry_id}\tlinknode={linknode}")
        } else {
            format!("{entry_id}")
        };
        if entry.subentries.is_empty() {
            ListItem::File(path, desc)
        } else {
            ListItem::Directory(path, desc)
        }
    }
}

impl std::fmt::Display for ListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListItem::Directory(path, entry) => {
                write!(f, "{path}/\t{entry}")?;
            }
            ListItem::File(path, entry) => {
                write!(f, "{path}\t{entry}")?;
            }
        }
        Ok(())
    }
}

trait Listable {
    fn list_item(self) -> String;
}

impl Listable for Entry<ContentManifestId, ContentManifestFile> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(file) => {
                format!(
                    "{}\ttype={}\tsize={}",
                    file.content_id, file.file_type, file.size
                )
            }
        }
    }
}

impl Listable for Entry<SkeletonManifestId, ()> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(()) => String::from("exists"),
        }
    }
}

impl Listable for Entry<SkeletonManifestV2, ()> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => format!("tree\tcount={}", tree.rollup_count().into_inner()),
            Entry::Leaf(()) => String::from("file"),
        }
    }
}

fn format_cluster_info(primary: &Option<MPath>, secondaries: &Option<Vec<MPath>>) -> String {
    let mut parts = Vec::new();
    if let Some(p) = primary {
        parts.push(format!("primary={p}"));
    }
    if let Some(s) = secondaries {
        if !s.is_empty() {
            let paths: Vec<_> = s.iter().map(|p| p.to_string()).collect();
            parts.push(format!("secondaries=[{}]", paths.join(", ")));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        parts.join("\t")
    }
}

impl Listable for Entry<DirectoryBranchClusterManifest, DirectoryBranchClusterManifestFile> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => {
                let cluster = format_cluster_info(&tree.primary, &tree.secondaries);
                if cluster.is_empty() {
                    String::from("tree")
                } else {
                    format!("tree\t{cluster}")
                }
            }
            Entry::Leaf(file) => {
                let cluster = format_cluster_info(&file.primary, &file.secondaries);
                if cluster.is_empty() {
                    String::from("file")
                } else {
                    format!("file\t{cluster}")
                }
            }
        }
    }
}

impl Listable for Entry<FsnodeId, FsnodeFile> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(file) => format!(
                "{}\ttype={}\tsize={}",
                file.content_id(),
                file.file_type(),
                file.size()
            ),
        }
    }
}

impl Listable for Entry<ManifestUnodeId, FileUnodeId> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(file) => file.to_string(),
        }
    }
}

impl Listable for Entry<HgAugmentedManifestId, HgAugmentedFileLeafNode> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(leaf) => format!(
                "{}\ttype={}\tsize={}\tblake3={}\tsha1={}{}",
                leaf.filenode,
                leaf.file_type,
                leaf.total_size,
                leaf.content_blake3,
                leaf.content_sha1,
                match leaf.file_header_metadata {
                    None => String::new(),
                    Some(data) => format!("\tmetadata=<{} bytes>", data.len()),
                }
            ),
        }
    }
}

impl Listable for Entry<HgManifestId, (FileType, HgFileNodeId)> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf((file_type, filenode)) => format!("{filenode}\ttype={file_type}",),
        }
    }
}

impl Listable for Entry<GitTreeId, GitLeaf> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(GitTreeId(oid)) => oid.to_string(),
            Entry::Leaf(GitLeaf(oid, mode)) => format!("{oid}\tmode={mode:06o}"),
        }
    }
}

async fn list<TreeId>(
    ctx: &CoreContext,
    repo: &Repo,
    root_id: TreeId,
    path: MPath,
    directory: bool,
    recursive: bool,
) -> Result<BoxStream<'static, Result<ListItem>>>
where
    TreeId: StoreLoadable<RepoBlobstore> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<RepoBlobstore>>::Value:
        Manifest<RepoBlobstore, TreeId = TreeId> + Send + Sync,
    <<TreeId as StoreLoadable<RepoBlobstore>>::Value as Manifest<RepoBlobstore>>::Leaf:
        Clone + Send + Eq + Unpin,
    Entry<
        TreeId,
        <<TreeId as StoreLoadable<RepoBlobstore>>::Value as Manifest<RepoBlobstore>>::Leaf,
    >: Listable,
{
    if directory {
        let entry = root_id
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;
        let item = ListItem::new(path, entry);
        Ok(futures::stream::once(async move { Ok(item) }).boxed())
    } else if recursive {
        let stream = if let Some(path) = path.into_optional_non_root_path() {
            root_id.list_leaf_entries_under(ctx.clone(), repo.repo_blobstore().clone(), [path])
        } else {
            root_id.list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        }
        .map_ok(|(path, entry)| ListItem::new(path.into(), Entry::Leaf(entry)))
        .boxed();
        Ok(stream)
    } else {
        let entry = root_id
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;

        match entry {
            Entry::Tree(tree_id) => {
                cloned!(ctx);
                let blobstore = repo.repo_blobstore().clone();
                Ok(try_stream! {
                    let tree = tree_id.load(&ctx, &blobstore).await?;
                    let mut subentries = tree.list(&ctx, &blobstore).await?;
                    while let Some((elem, subentry)) = subentries.try_next().await? {
                        yield ListItem::new(path.join_element(Some(&elem)), subentry);
                    }
                }
                .boxed())
            }
            Entry::Leaf(..) => {
                Ok(futures::stream::once(async move { Ok(ListItem::new(path, entry)) }).boxed())
            }
        }
    }
}

/// Special listing function for DirectoryBranchClusterManifest (DBCM).
/// DBCM is a sparse manifest that doesn't store all directory entries, so we need
/// to traverse the tree manually rather than using list_leaf_entries.
async fn list_dbcm(
    ctx: &CoreContext,
    repo: &Repo,
    root: DirectoryBranchClusterManifest,
    path: MPath,
    directory: bool,
    recursive: bool,
) -> Result<BoxStream<'static, Result<ListItem>>> {
    if directory {
        let entry = root
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;
        let item = ListItem::new(path, entry);
        Ok(futures::stream::once(async move { Ok(item) }).boxed())
    } else if recursive {
        // For recursive listing, we traverse only existing entries in the manifest
        // This is important for sparse manifests like DBCM that don't store all directories
        list_dbcm_recursive(ctx, repo, root, path).await
    } else {
        let entry = root
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;

        match entry {
            Entry::Tree(tree_id) => {
                cloned!(ctx);
                let blobstore = repo.repo_blobstore().clone();
                Ok(try_stream! {
                    let tree = tree_id.load(&ctx, &blobstore).await?;
                    let mut subentries = tree.list(&ctx, &blobstore).await?;
                    while let Some((elem, subentry)) = subentries.try_next().await? {
                        yield ListItem::new(path.join_element(Some(&elem)), subentry);
                    }
                }
                .boxed())
            }
            Entry::Leaf(..) => {
                Ok(futures::stream::once(async move { Ok(ListItem::new(path, entry)) }).boxed())
            }
        }
    }
}

/// Recursively list all entries in a manifest by traversing only existing subentries.
/// This is specifically designed for sparse manifests like DBCM that don't store all directories.
/// Unlike the standard recursive listing that uses list_leaf_entries (which only returns files),
/// this function yields both directories and files by manually traversing the tree structure.
async fn list_dbcm_recursive<TreeId>(
    ctx: &CoreContext,
    repo: &Repo,
    root_id: TreeId,
    start_path: MPath,
) -> Result<BoxStream<'static, Result<ListItem>>>
where
    TreeId: StoreLoadable<RepoBlobstore> + Clone + Send + Sync + Eq + Unpin + 'static,
    <TreeId as StoreLoadable<RepoBlobstore>>::Value:
        Manifest<RepoBlobstore, TreeId = TreeId> + Send + Sync,
    <<TreeId as StoreLoadable<RepoBlobstore>>::Value as Manifest<RepoBlobstore>>::Leaf:
        Clone + Send + Eq + Unpin,
    Entry<
        TreeId,
        <<TreeId as StoreLoadable<RepoBlobstore>>::Value as Manifest<RepoBlobstore>>::Leaf,
    >: Listable,
{
    cloned!(ctx);
    let blobstore = repo.repo_blobstore().clone();

    Ok(try_stream! {
        // Start at the given path
        let entry = root_id
            .find_entry(ctx.clone(), blobstore.clone(), start_path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{start_path}'"))?;

        match entry {
            Entry::Tree(tree_id) => {
                // Use a stack to traverse the tree iteratively
                let mut stack = vec![(start_path.clone(), tree_id)];

                while let Some((current_path, current_tree_id)) = stack.pop() {
                    let tree = current_tree_id.load(&ctx, &blobstore).await?;
                    let mut subentries = tree.list(&ctx, &blobstore).await?;

                    while let Some((elem, subentry)) = subentries.try_next().await? {
                        let subpath = current_path.join_element(Some(&elem));

                        match subentry {
                            Entry::Tree(subtree_id) => {
                                // Add to stack for further traversal
                                stack.push((subpath.clone(), subtree_id.clone()));
                                // Yield this directory
                                yield ListItem::new(subpath, Entry::Tree(subtree_id));
                            }
                            Entry::Leaf(leaf) => {
                                // Yield this leaf
                                yield ListItem::new(subpath, Entry::Leaf(leaf));
                            }
                        }
                    }
                }
            }
            Entry::Leaf(leaf) => {
                // Just a single leaf
                yield ListItem::new(start_path, Entry::Leaf(leaf));
            }
        }
    }
    .boxed())
}

/// Custom listing function for AclManifest.
/// AclManifest is a sparse manifest using ShardedMapV2 that only stores restriction
/// roots and their ancestors. We work with AclManifestEntry directly to show
/// ACL-specific metadata (is_restricted, has_restricted_descendants).
async fn list_acl(
    ctx: &CoreContext,
    repo: &Repo,
    root_id: AclManifestId,
    path: MPath,
    directory: bool,
    recursive: bool,
) -> Result<BoxStream<'static, Result<ListItem>>> {
    let blobstore = repo.repo_blobstore().clone();
    let root: AclManifest = root_id.load(ctx, &blobstore).await?;

    // Navigate to the target path
    let mut current = root;
    for elem in path.clone().into_iter() {
        let entry = current
            .lookup(ctx, &blobstore, &elem)
            .await?
            .ok_or_else(|| anyhow!("No ACL manifest for path '{path}'"))?;
        match entry {
            AclManifestEntry::Directory(dir) => {
                current = dir.id.load(ctx, &blobstore).await?;
            }
            AclManifestEntry::AclFile(_) => {
                if directory {
                    return Ok(futures::stream::once(async move {
                        Ok(ListItem::File(path, format_acl_entry(&entry)))
                    })
                    .boxed());
                }
                return Err(anyhow!("Path '{path}' is a leaf, not a directory"));
            }
        }
    }

    if directory {
        let desc = format_acl_restriction(&current.restriction);
        return Ok(
            futures::stream::once(async move { Ok(ListItem::Directory(path, desc)) }).boxed(),
        );
    }

    if recursive {
        return list_acl_recursive(ctx, repo, current, path).await;
    }

    // List immediate children
    let items: Vec<ListItem> = current
        .into_subentries(ctx, &blobstore)
        .map_ok(|(elem, entry)| {
            let subpath = path.join_element(Some(&elem));
            let desc = format_acl_entry(&entry);
            match entry {
                AclManifestEntry::Directory(_) => ListItem::Directory(subpath, desc),
                AclManifestEntry::AclFile(_) => ListItem::File(subpath, desc),
            }
        })
        .try_collect()
        .await?;

    Ok(futures::stream::iter(items.into_iter().map(Ok)).boxed())
}

async fn list_acl_recursive(
    ctx: &CoreContext,
    repo: &Repo,
    root: AclManifest,
    start_path: MPath,
) -> Result<BoxStream<'static, Result<ListItem>>> {
    cloned!(ctx);
    let blobstore = repo.repo_blobstore().clone();

    Ok(try_stream! {
        let mut stack: Vec<(MPath, AclManifest)> = vec![(start_path, root)];

        while let Some((current_path, manifest)) = stack.pop() {
            let mut subentries = manifest.into_subentries(&ctx, &blobstore);
            while let Some((elem, entry)) = subentries.try_next().await? {
                let subpath = current_path.join_element(Some(&elem));
                let desc = format_acl_entry(&entry);
                match entry {
                    AclManifestEntry::Directory(dir) => {
                        yield ListItem::Directory(subpath.clone(), desc);
                        let child: AclManifest = dir.id.load(&ctx, &blobstore).await?;
                        stack.push((subpath, child));
                    }
                    AclManifestEntry::AclFile(_) => {
                        yield ListItem::File(subpath, desc);
                    }
                }
            }
        }
    }
    .boxed())
}

fn format_acl_restriction(restriction: &AclManifestDirectoryRestriction) -> String {
    match restriction {
        AclManifestDirectoryRestriction::Unrestricted => String::from("unrestricted"),
        AclManifestDirectoryRestriction::Restricted(r) => {
            format!("restricted\tentry_blob={}", r.entry_blob_id)
        }
    }
}

fn format_acl_entry(entry: &AclManifestEntry) -> String {
    match entry {
        AclManifestEntry::AclFile(restriction) => {
            format!("acl_file\tentry_blob={}", restriction.entry_blob_id)
        }
        AclManifestEntry::Directory(dir) => {
            format!(
                "directory\tid={}\tis_restricted={}\thas_restricted_descendants={}",
                dir.id, dir.is_restricted, dir.has_restricted_descendants
            )
        }
    }
}

/// Same as `list`, but for deleted manifests, which are structured differently and have their own `Ops`.
async fn list_deleted(
    ctx: &CoreContext,
    repo: &Repo,
    root_id: RootDeletedManifestV2Id,
    path: MPath,
    directory: bool,
    recursive: bool,
) -> Result<BoxStream<'static, Result<ListItem>>> {
    cloned!(ctx);
    let blobstore = repo.repo_blobstore().clone();
    if directory {
        // Find and load the deleted manifest for this path.
        let mut entry_id = *root_id.id();
        for elem in path.clone().into_iter() {
            let entry = entry_id.load(&ctx, &blobstore).await?;
            entry_id = entry
                .subentries
                .lookup(&ctx, &blobstore, elem.as_ref())
                .await?
                .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;
        }
        let entry = entry_id.load(&ctx, &blobstore).await?;
        // See if the path itself is deleted, and yield a "file" entry for this path if so.
        let item = ListItem::new_deleted(path, entry_id, entry);
        Ok(futures::stream::once(async move { Ok(item) }).boxed())
    } else if recursive {
        let stream = try_stream! {
            let mut entries = root_id.find_entries(&ctx, &blobstore, vec![PathOrPrefix::Prefix(path)]);

            while let Some((path, entry_id)) = entries.try_next().await? {
                cloned!(ctx, blobstore);
                yield async move {
                    let entry = entry_id.load(&ctx, &blobstore).await?;
                    Ok(ListItem::new_deleted(path, entry_id, entry))
                }
            }
        }
        .try_buffered(100)
        .boxed();
        Ok(stream)
    } else {
        // DeletedManifestOps only supports listing the deleted paths, and doesn't support listing
        // directories that aren't themselves also deleted.  Instead, we will find the
        // directory manifest and examine it directly.
        let stream = try_stream! {
            // Find and load the deleted manifest for this path.
            let mut entry_id = *root_id.id();
            for elem in path.clone().into_iter() {
                let entry = entry_id.load(&ctx, &blobstore).await?;
                entry_id = entry
                    .subentries
                    .lookup(&ctx, &blobstore, elem.as_ref())
                    .await?
                    .ok_or_else(|| anyhow!("No manifest for path '{path}'"))?;
            }
            let entry = entry_id.load(&ctx, &blobstore).await?;
            // Work through the subentries of this directory, yielding a "directory" entry for
            // subentries that have their own subentries, and a "file" entry for subentries that
            // are deleted.
            let mut subentries = entry.subentries.into_entries(&ctx, &blobstore);
            while let Some((elem, subentry_id)) = subentries.try_next().await? {
                cloned!(ctx, blobstore, path);
                yield async move {
                    let subentry = subentry_id.load(&ctx, &blobstore).await?;
                    let subpath = path.join_element(Some(&MPathElement::from_smallvec(elem)?));
                    let item = ListItem::new_deleted(subpath, subentry_id, subentry);
                    Ok(item)
                };
            }
        }
        .try_buffered(100)
        .boxed();

        Ok(stream)
    }
}

/// Format a history manifest entry by loading the underlying blob.
async fn format_history_entry(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    entry: &HistoryManifestEntry,
) -> Result<String> {
    match entry {
        HistoryManifestEntry::File(id) => {
            let file: HistoryManifestFile = id.load(ctx, blobstore).await?;
            Ok(format!(
                "File\tlinknode={}\tparents={}",
                file.linknode,
                file.parents.len()
            ))
        }
        HistoryManifestEntry::Directory(id) => {
            let dir: HistoryManifestDirectory = id.load(ctx, blobstore).await?;
            Ok(format!(
                "Directory\tlinknode={}\tparents={}",
                dir.linknode,
                dir.parents.len()
            ))
        }
        HistoryManifestEntry::DeletedNode(deleted) => {
            let node: HistoryManifestDeletedNode = deleted.load(ctx, blobstore).await?;
            Ok(format!(
                "DeletedNode\tlinknode={}\tparents={}",
                node.linknode,
                node.parents.len()
            ))
        }
    }
}

/// List the contents of a history manifest.
async fn list_history_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    root_dir: HistoryManifestDirectory,
    path: MPath,
    directory: bool,
    recursive: bool,
) -> Result<BoxStream<'static, Result<ListItem>>> {
    let blobstore = repo.repo_blobstore().clone();

    if directory {
        // Navigate to the requested path and display that entry.
        let entry = find_history_entry(ctx, &blobstore, &root_dir, &path).await?;
        let desc = format_history_entry(ctx, &blobstore, &entry).await?;
        let item = ListItem::new_history_manifest(path, &entry, desc);
        Ok(futures::stream::once(async move { Ok(item) }).boxed())
    } else if recursive {
        // Recursively walk all entries.
        cloned!(ctx, blobstore);
        Ok(try_stream! {
            let start_dir = if path.is_root() {
                root_dir
            } else {
                let entry = find_history_entry(&ctx, &blobstore, &root_dir, &path).await?;
                match entry {
                    HistoryManifestEntry::Directory(id) => {
                        id.load(&ctx, &blobstore).await?
                    }
                    _ => {
                        let desc = format_history_entry(&ctx, &blobstore, &entry).await?;
                        yield ListItem::new_history_manifest(path, &entry, desc);
                        return;
                    }
                }
            };

            let mut stack: Vec<(MPath, HistoryManifestDirectory)> = vec![(path, start_dir)];

            while let Some((current_path, dir)) = stack.pop() {
                let subentries: Vec<_> = dir
                    .into_subentries(&ctx, &blobstore)
                    .try_collect()
                    .await?;

                for (name, entry) in subentries {
                    let subpath = current_path.join(&name);
                    let desc = format_history_entry(&ctx, &blobstore, &entry).await?;
                    yield ListItem::new_history_manifest(subpath.clone(), &entry, desc);

                    match entry {
                        HistoryManifestEntry::Directory(id) => {
                            let child_dir: HistoryManifestDirectory = id.load(&ctx, &blobstore).await?;
                            stack.push((subpath, child_dir));
                        }
                        HistoryManifestEntry::DeletedNode(deleted) => {
                            let node: HistoryManifestDeletedNode = deleted.load(&ctx, &blobstore).await?;
                            // Walk deleted node subentries recursively.
                            let node_subs: Vec<_> = node.into_subentries(&ctx, &blobstore).try_collect().await?;
                            for (sub_name, sub_entry) in node_subs {
                                let sub_path = subpath.join(&sub_name);
                                let sub_desc = format_history_entry(&ctx, &blobstore, &sub_entry).await?;
                                yield ListItem::new_history_manifest(sub_path.clone(), &sub_entry, sub_desc);
                                if let HistoryManifestEntry::Directory(sub_id) = sub_entry {
                                    let sub_dir: HistoryManifestDirectory = sub_id.load(&ctx, &blobstore).await?;
                                    stack.push((sub_path, sub_dir));
                                }
                            }
                        }
                        HistoryManifestEntry::File(id) => {
                            let file: HistoryManifestFile = id.load(&ctx, &blobstore).await?;
                            // Walk file subentries (file-replaces-directory case).
                            let file_subs: Vec<_> = file.into_subentries(&ctx, &blobstore).try_collect().await?;
                            for (sub_name, sub_entry) in file_subs {
                                let sub_path = subpath.join(&sub_name);
                                let sub_desc = format_history_entry(&ctx, &blobstore, &sub_entry).await?;
                                yield ListItem::new_history_manifest(sub_path.clone(), &sub_entry, format!("(file-sub) {sub_desc}"));
                                if let HistoryManifestEntry::Directory(sub_id) = sub_entry {
                                    let sub_dir: HistoryManifestDirectory = sub_id.load(&ctx, &blobstore).await?;
                                    stack.push((sub_path, sub_dir));
                                }
                            }
                        }
                    }
                }
            }
        }
        .boxed())
    } else {
        // List immediate subentries of the directory at the given path.
        let dir = if path.is_root() {
            root_dir
        } else {
            let entry = find_history_entry(ctx, &blobstore, &root_dir, &path).await?;
            match entry {
                HistoryManifestEntry::Directory(id) => id.load(ctx, &blobstore).await?,
                _ => {
                    let desc = format_history_entry(ctx, &blobstore, &entry).await?;
                    let item = ListItem::new_history_manifest(path, &entry, desc);
                    return Ok(futures::stream::once(async move { Ok(item) }).boxed());
                }
            }
        };

        cloned!(ctx, blobstore);
        Ok(try_stream! {
            let subentries: Vec<_> = dir
                .into_subentries(&ctx, &blobstore)
                .try_collect()
                .await?;
            for (name, entry) in subentries {
                let subpath = path.join(&name);
                let desc = format_history_entry(&ctx, &blobstore, &entry).await?;
                yield ListItem::new_history_manifest(subpath, &entry, desc);
            }
        }
        .boxed())
    }
}

/// Navigate into a history manifest directory to find the entry at a given path.
async fn find_history_entry(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    root_dir: &HistoryManifestDirectory,
    path: &MPath,
) -> Result<HistoryManifestEntry> {
    let elements: Vec<_> = path.clone().into_iter().collect();
    if elements.is_empty() {
        return Err(anyhow!("Cannot look up entry at root path"));
    }

    let mut current_dir = root_dir.clone();
    for (i, elem) in elements.iter().enumerate() {
        let entry = current_dir
            .lookup(ctx, blobstore, elem)
            .await?
            .ok_or_else(|| anyhow!("No entry for path '{path}' in history manifest"))?;

        if i == elements.len() - 1 {
            return Ok(entry);
        }

        // Need to descend into a directory for the next element.
        match entry {
            HistoryManifestEntry::Directory(id) => {
                current_dir = id.load(ctx, blobstore).await?;
            }
            _ => {
                return Err(anyhow!(
                    "Expected directory at intermediate path component '{elem}' in '{path}'"
                ));
            }
        }
    }

    Err(anyhow!("Unexpected end of path traversal for '{path}'"))
}

async fn fetch_or_derive_root<TreeId>(
    ctx: &CoreContext,
    manager: &DerivedDataManager,
    cs_id: ChangesetId,
    derive: bool,
) -> Result<TreeId>
where
    TreeId: BonsaiDerivable,
{
    if derive {
        Ok(manager
            .derive::<TreeId>(ctx, cs_id, None, DerivationPriority::LOW)
            .await?)
    } else {
        manager
            .fetch_derived::<TreeId>(ctx, cs_id, None)
            .await?
            .ok_or_else(|| anyhow!("No manifest for changeset {cs_id}"))
    }
}

pub(super) async fn list_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: ListManifestArgs,
) -> Result<()> {
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;

    let path = args.path.as_deref().unwrap_or("");
    let path: MPath = MPath::new(path).with_context(|| anyhow!("Invalid path: {path}"))?;

    let items = match args.manifest_type {
        ListManifestType::SkeletonManifests => {
            let root_id =
                fetch_or_derive_root::<RootSkeletonManifestId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .into_skeleton_manifest_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::SkeletonManifests2 => {
            let root =
                fetch_or_derive_root::<RootSkeletonManifestV2Id>(ctx, manager, cs_id, args.derive)
                    .await?
                    .into_inner_id()
                    .load(ctx, repo.repo_blobstore())
                    .await?;
            list(ctx, repo, root, path, args.directory, args.recursive).await?
        }
        ListManifestType::ContentManifests => {
            let root_id =
                fetch_or_derive_root::<RootContentManifestId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .into_content_manifest_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::DirectoryBranchClusterManifests => {
            let root = fetch_or_derive_root::<RootDirectoryBranchClusterManifestId>(
                ctx,
                manager,
                cs_id,
                args.derive,
            )
            .await?
            .into_inner_id()
            .load(ctx, repo.repo_blobstore())
            .await?;
            list_dbcm(ctx, repo, root, path, args.directory, args.recursive).await?
        }
        ListManifestType::Fsnodes => {
            let root_id = fetch_or_derive_root::<RootFsnodeId>(ctx, manager, cs_id, args.derive)
                .await?
                .into_fsnode_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::Unodes => {
            let root_id =
                *fetch_or_derive_root::<RootUnodeManifestId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .manifest_unode_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::HgManifests => {
            let hg_changeset_id =
                fetch_or_derive_root::<MappedHgChangesetId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .hg_changeset_id();
            let root_id = hg_changeset_id
                .load(ctx, repo.repo_blobstore())
                .await?
                .manifestid();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::HgAugmentedManifests => {
            let root_id =
                fetch_or_derive_root::<RootHgAugmentedManifestId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .hg_augmented_manifest_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::DeletedManifests => {
            let root_id =
                fetch_or_derive_root::<RootDeletedManifestV2Id>(ctx, manager, cs_id, args.derive)
                    .await?;
            list_deleted(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::GitTrees => {
            let root_id =
                fetch_or_derive_root::<MappedGitCommitId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .fetch_root_tree(ctx, repo.repo_blobstore())
                    .await?;
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::AclManifests => {
            let root_id =
                fetch_or_derive_root::<RootAclManifestId>(ctx, manager, cs_id, args.derive)
                    .await?
                    .into_inner_id();
            list_acl(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::HistoryManifests => {
            let root_dir = fetch_or_derive_root::<RootHistoryManifestDirectoryId>(
                ctx,
                manager,
                cs_id,
                args.derive,
            )
            .await?
            .into_history_manifest_directory_id()
            .load(ctx, repo.repo_blobstore())
            .await?;
            list_history_manifest(ctx, repo, root_dir, path, args.directory, args.recursive).await?
        }
    };

    items
        .try_for_each(|item| async move {
            println!("{item}");
            Ok(())
        })
        .await?;

    Ok(())
}
