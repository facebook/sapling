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
use either::Either;
use futures::TryStreamExt;
use manifest::Entry;
use manifest::Manifest;
use mononoke_types::content_manifest::ContentManifest;
use mononoke_types::content_manifest::compat;
use mononoke_types::fsnode::Fsnode;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a manifest (either ContentManifest or Fsnode), with a summary
/// header and one entry per line.
pub async fn display_manifest<B: KeyedBlobstore>(
    mut w: impl Write,
    ctx: &CoreContext,
    blobstore: &B,
    manifest: Either<ContentManifest, Fsnode>,
) -> Result<()> {
    // Summary — each manifest type stores this differently.
    match &manifest {
        Either::Left(cm) => {
            let rollup = cm.subentries.rollup_data();
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
        }
        Either::Right(fsnode) => {
            let summary = fsnode.summary();
            writeln!(w, "Summary:")?;
            writeln!(
                w,
                "Children: {} files ({}), {} dirs",
                summary.child_files_count, summary.child_files_total_size, summary.child_dirs_count
            )?;
            writeln!(
                w,
                "Descendants: {} files ({})",
                summary.descendant_files_count, summary.descendant_files_total_size
            )?;
        }
    }

    // Children list — unified via the Manifest trait on Either.
    writeln!(w, "Children list:")?;
    let entries: Vec<_> = Manifest::list(&manifest, ctx, blobstore)
        .await?
        .map_ok(|(name, entry)| {
            let name = String::from_utf8_lossy(name.as_ref()).into_owned();
            let (id, ty) = match entry {
                Entry::Leaf(leaf) => {
                    let file: compat::ContentManifestFile = leaf.into();
                    (file.content_id().to_string(), file.file_type().to_string())
                }
                Entry::Tree(tree_id) => (
                    tree_id.either(|id| id.to_string(), |id| id.to_string()),
                    "tree".to_string(),
                ),
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
