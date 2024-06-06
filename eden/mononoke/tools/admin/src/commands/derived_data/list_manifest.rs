/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_stream::try_stream;
use clap::Args;
use clap::ValueEnum;
use cloned::cloned;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable;
use fsnodes::RootFsnodeId;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::AsyncManifest;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
use manifest::StoreLoadable;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_app::args::ChangesetArgs;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::deleted_manifest_v2::DeletedManifestV2;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::FileType;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use skeleton_manifest::RootSkeletonManifestId;
use unodes::RootUnodeManifestId;

use super::Repo;

/// Supported manifest types
#[derive(Copy, Clone, Debug, ValueEnum)]
enum ListManifestType {
    SkeletonManifests,
    Fsnodes,
    Unodes,
    DeletedManifests,
    HgManifests,
    HgAugmentedManifests,
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
    fn new<TreeId, LeafId>(path: MPath, entry: Entry<TreeId, LeafId>) -> Self
    where
        Entry<TreeId, LeafId>: Listable,
    {
        match entry {
            Entry::Tree(..) => ListItem::Directory(path, entry.list_item()),
            Entry::Leaf(..) => ListItem::File(path, entry.list_item()),
        }
    }

    fn new_deleted(path: MPath, entry_id: DeletedManifestV2Id, entry: DeletedManifestV2) -> Self {
        let desc = if let Some(linknode) = entry.linknode() {
            format!("{}\tlinknode={}", entry_id, linknode)
        } else {
            format!("{}", entry_id)
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
                write!(f, "{}/\t{}", path, entry)?;
            }
            ListItem::File(path, entry) => {
                write!(f, "{}\t{}", path, entry)?;
            }
        }
        Ok(())
    }
}

trait Listable {
    fn list_item(self) -> String;
}

impl Listable for Entry<SkeletonManifestId, ()> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf(()) => String::from("exists"),
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
                "{}\ttype={}\tsize={}\tblake3={}\tsha1={}",
                leaf.filenode,
                leaf.file_type,
                leaf.total_size,
                leaf.content_blake3,
                leaf.content_sha1,
            ),
        }
    }
}

impl Listable for Entry<HgManifestId, (FileType, HgFileNodeId)> {
    fn list_item(self) -> String {
        match self {
            Entry::Tree(tree) => tree.to_string(),
            Entry::Leaf((file_type, filenode)) => format!("{}\ttype={}", filenode, file_type,),
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
        AsyncManifest<RepoBlobstore, TreeId = TreeId> + Send + Sync,
    <<TreeId as StoreLoadable<RepoBlobstore>>::Value as AsyncManifest<RepoBlobstore>>::LeafId:
        Clone + Send + Eq + Unpin,
    Entry<
        TreeId,
        <<TreeId as StoreLoadable<RepoBlobstore>>::Value as AsyncManifest<RepoBlobstore>>::LeafId,
    >: Listable,
{
    if directory {
        let entry = root_id
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path.clone())
            .await?
            .ok_or_else(|| anyhow!("No manifest for path '{}'", path))?;
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
            .ok_or_else(|| anyhow!("No manifest for path '{}'", path))?;

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
                .ok_or_else(|| anyhow!("No manifest for path '{}'", path))?;
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
                    .ok_or_else(|| anyhow!("No manifest for path '{}'", path))?;
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

async fn fetch_or_derive_root<TreeId>(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    derive: bool,
) -> Result<TreeId>
where
    TreeId: BonsaiDerivable,
{
    if derive {
        Ok(repo
            .repo_derived_data()
            .derive::<TreeId>(ctx, cs_id)
            .await?)
    } else {
        repo.repo_derived_data()
            .fetch_derived::<TreeId>(ctx, cs_id)
            .await?
            .ok_or_else(|| anyhow!("No manifest for changeset {}", cs_id))
    }
}

pub(super) async fn list_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    args: ListManifestArgs,
) -> Result<()> {
    let cs_id = args
        .changeset_args
        .resolve_changeset(ctx, repo)
        .await?
        .ok_or_else(|| anyhow!("Changeset not found"))?;

    let path = args.path.as_deref().unwrap_or("");
    let path: MPath = MPath::new(path).with_context(|| anyhow!("Invalid path: {}", path))?;

    let items = match args.manifest_type {
        ListManifestType::SkeletonManifests => {
            let root_id =
                fetch_or_derive_root::<RootSkeletonManifestId>(ctx, repo, cs_id, args.derive)
                    .await?
                    .into_skeleton_manifest_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::Fsnodes => {
            let root_id = fetch_or_derive_root::<RootFsnodeId>(ctx, repo, cs_id, args.derive)
                .await?
                .into_fsnode_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::Unodes => {
            let root_id =
                *fetch_or_derive_root::<RootUnodeManifestId>(ctx, repo, cs_id, args.derive)
                    .await?
                    .manifest_unode_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::HgManifests => {
            let hg_changeset_id =
                fetch_or_derive_root::<MappedHgChangesetId>(ctx, repo, cs_id, args.derive)
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
                fetch_or_derive_root::<RootHgAugmentedManifestId>(ctx, repo, cs_id, args.derive)
                    .await?
                    .hg_augmented_manifest_id();
            list(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
        ListManifestType::DeletedManifests => {
            let root_id =
                fetch_or_derive_root::<RootDeletedManifestV2Id>(ctx, repo, cs_id, args.derive)
                    .await?;
            list_deleted(ctx, repo, root_id, path, args.directory, args.recursive).await?
        }
    };

    items
        .try_for_each(|item| async move {
            println!("{}", item);
            Ok(())
        })
        .await?;

    Ok(())
}
