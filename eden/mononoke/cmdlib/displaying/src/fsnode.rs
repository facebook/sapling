/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeEntry;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

/// Displays a fanode manifest, one entry per line.
pub fn display_fsnode_manifest(mut w: impl Write, fsnode: &Fsnode) -> Result<()> {
    writeln!(w, "Summary:",)?;
    let summary = fsnode.summary();
    writeln!(w, "Simple-Format-SHA1: {}", summary.simple_format_sha1)?;
    writeln!(w, "Simple-Format-SHA256: {}", summary.simple_format_sha256)?;
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

    writeln!(w, "Children list:",)?;
    let entries = fsnode
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
            FsnodeEntry::File(fsnode_file) => (
                fsnode_file.file_type().to_string(),
                fsnode_file.content_id().to_string(),
            ),
            FsnodeEntry::Directory(fsnode_dir) => ("tree".to_string(), fsnode_dir.id().to_string()),
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
