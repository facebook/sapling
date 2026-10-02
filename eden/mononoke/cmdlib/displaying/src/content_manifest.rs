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
use futures::TryStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mononoke_types::content_manifest::ContentManifest;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a content manifest with a summary
/// header and one entry per line.
pub async fn display_manifest<B: KeyedBlobstore>(
    mut w: impl Write,
    ctx: &CoreContext,
    blobstore: &B,
    manifest: ContentManifest,
) -> Result<()> {
    let rollup = manifest.subentries.rollup_data();
    writeln!(w, "Summary:")?;
    writeln!(
        w,
        "Children: {} files ({}), {} dirs",
        rollup.child_counts.files_count,
        rollup.child_counts.files_total_size,
        rollup.child_counts.dirs_count
    )?;
    writeln!(
        w,
        "Descendants: {} files ({}), {} dirs",
        rollup.descendant_counts.files_count,
        rollup.descendant_counts.files_total_size,
        rollup.descendant_counts.dirs_count
    )?;

    writeln!(w, "Children list:")?;
    let entries: Vec<_> = Manifest::list(&manifest, ctx, blobstore)
        .await?
        .map_ok(|(name, entry)| {
            let name = String::from_utf8_lossy(name.as_ref()).into_owned();
            let (id, ty) = match entry {
                Entry::Leaf(file) => (file.content_id.to_string(), file.file_type.to_string()),
                Entry::Tree(tree_id) => (tree_id.to_string(), "tree".to_string()),
            };
            (name, id, ty)
        })
        .try_collect()
        .await?;

    let max_width = entries
        .iter()
        .map(|(name, _, _)| name.width())
        .max()
        .unwrap_or(0);
    for (name, id, ty) in entries {
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
