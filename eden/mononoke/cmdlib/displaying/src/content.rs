/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;

use crate::hexdump;

/// Displays content, handling the possibility of binary content.
///
/// Non-UTF-8 content is displayed as a hex-dump of the first 1024 bytes.
/// UTF-8 content is preceded by a newline, so it is distinct from any
/// hexdump.
pub fn display_content(mut w: impl Write, content: impl AsRef<[u8]>) -> Result<()> {
    let content = content.as_ref();
    if let Ok(utf8_content) = std::str::from_utf8(content) {
        writeln!(w, "\n{}", utf8_content)?;
    } else {
        writeln!(w, "Hexdump (first 1024 bytes):")?;
        hexdump(w, &content[..content.len().min(1024)])?;
    }
    Ok(())
}
