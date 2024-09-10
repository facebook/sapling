/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::blobs::HgBlobManifest;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a Mercurial manifest, one entry per line.
pub async fn display_hg_manifest(
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    mut w: impl Write,
    manifest: &HgBlobManifest,
) -> Result<()> {
    let entries = manifest
        .list(ctx, blobstore)
        .await?
        .map_ok(|(name, entry)| (String::from_utf8_lossy(name.as_ref()).into_owned(), entry))
        .try_collect::<Vec<_>>()
        .await?;
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
