/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::blobs::HgBlobManifest;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a Mercurial manifest, one entry per line.
pub fn display_hg_manifest(mut w: impl Write, manifest: &HgBlobManifest) -> Result<()> {
    let entries = manifest
        .list()
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
