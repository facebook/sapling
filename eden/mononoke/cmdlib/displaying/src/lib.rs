/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod changeset;
mod content;
mod mercurial;

use std::io::Write;

use anyhow::Result;
pub use changeset::DisplayChangeset;
pub use content::display_content;
pub use mercurial::display_hg_manifest;

/// Hexdump a block of data to the output stream.
pub fn hexdump(mut w: impl Write, data: impl AsRef<[u8]>) -> Result<()> {
    const CHUNK_SIZE: usize = 16;

    fn sanitize(slice: &[u8]) -> String {
        slice
            .iter()
            .map(|c| {
                if *c < b' ' || *c > b'~' {
                    '.'
                } else {
                    *c as char
                }
            })
            .collect::<String>()
    }

    for (i, slice) in data.as_ref().chunks(CHUNK_SIZE).enumerate() {
        writeln!(
            w,
            "{:08x}: {:<32}  {}",
            i * CHUNK_SIZE,
            hex::encode(slice),
            sanitize(slice),
        )?;
    }
    Ok(())
}
