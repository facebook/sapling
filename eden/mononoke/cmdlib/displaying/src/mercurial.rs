/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::FileType;
use mercurial_types::HgAugmentedManifestEnvelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::MPathElement;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::sharded_augmented_manifest::hg_entry_from_augmented;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a Mercurial manifest, one entry per line.
pub async fn display_hg_manifest(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    w: impl Write,
    manifest: &HgBlobManifest,
) -> Result<()> {
    let entries = manifest.list(ctx, blobstore).await?.try_collect().await?;
    write_hg_manifest_entries(w, entries)
}

/// Displays the Mercurial manifest that an augmented manifest describes, in
/// the same format as `display_hg_manifest`.
pub async fn display_hg_augmented_manifest(
    ctx: &CoreContext,
    blobstore: &impl KeyedBlobstore,
    w: impl Write,
    manifest: &HgAugmentedManifestEnvelope,
) -> Result<()> {
    let entries = manifest
        .list(ctx, blobstore)
        .await?
        .map_ok(|(name, entry)| (name, hg_entry_from_augmented(entry)))
        .try_collect()
        .await?;
    write_hg_manifest_entries(w, entries)
}

fn write_hg_manifest_entries(
    mut w: impl Write,
    entries: Vec<(MPathElement, Entry<HgManifestId, (FileType, HgFileNodeId)>)>,
) -> Result<()> {
    let entries = entries
        .into_iter()
        .map(|(name, entry)| (String::from_utf8_lossy(name.as_ref()).into_owned(), entry))
        .collect::<Vec<_>>();
    let max_width = entries
        .iter()
        .map(|(name, _)| name.width())
        .max()
        .unwrap_or(0);
    for (name, entry) in entries {
        let (ty, id) = match entry {
            Entry::Leaf((ty, id)) => (ty.to_string(), id.to_string()),
            Entry::Tree(id) => ("tree".to_string(), id.to_string()),
        };
        writeln!(
            w,
            "{} {} {}",
            name.unicode_pad(max_width, Alignment::Left, false),
            id,
            ty,
        )?;
    }
    Ok(())
}
