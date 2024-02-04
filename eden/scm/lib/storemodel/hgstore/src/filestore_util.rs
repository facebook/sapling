/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! See https://repo.mercurial-scm.org/hg/rev/10519e4cbd02 for the "hg file metadata" format.

use std::str;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Result;
use minibytes::Bytes;
use types::HgId;
use types::Key;
use types::RepoPath;

/// Mercurial may embed the copy-from information into the blob itself, in which case, the `Delta`
/// would look like:
///
///   \1
///   copy: path
///   copyrev: sha1
///   \1
///   blob
///
/// If the blob starts with \1\n too, it's escaped by adding \1\n\1\n at the beginning.
pub fn strip_hg_file_metadata(data: &Bytes) -> Result<(Bytes, Option<Key>)> {
    let (blob, copy_from) = split_hg_file_metadata(data)?;
    if copy_from.len() > 0 {
        let slice = copy_from.as_ref();
        let slice = &slice[2..copy_from.len() - 2];
        let mut path = None;
        let mut hgid = None;

        for line in slice.split(|c| c == &b'\n') {
            if line.is_empty() {
                continue;
            }
            if line.starts_with(b"copy: ") {
                path = Some(RepoPath::from_str(str::from_utf8(&line[6..])?)?.to_owned());
            } else if line.starts_with(b"copyrev: ") {
                hgid = Some(HgId::from_str(str::from_utf8(&line[9..])?)?);
            } else {
                bail!("Unknown metadata in data: {:?}", line);
            }
        }

        let key = match (path, hgid) {
            (None, Some(_)) => bail!("missing 'copyrev' metadata"),
            (Some(_), None) => bail!("missing 'copy' metadata"),

            (None, None) => None,
            (Some(path), Some(hgid)) => Some(Key::new(path, hgid)),
        };

        Ok((blob, key))
    } else {
        Ok((blob, None))
    }
}

pub fn split_hg_file_metadata(data: &Bytes) -> Result<(Bytes, Bytes)> {
    let slice = data.as_ref();
    if !slice.starts_with(b"\x01\n") {
        return Ok((data.clone(), Bytes::new()));
    }
    let slice = &slice[2..];
    if let Some(pos) = slice.windows(2).position(|needle| needle == b"\x01\n") {
        let split_pos = 2 + pos + 2;
        Ok((data.slice(split_pos..), data.slice(..split_pos)))
    } else {
        Ok((data.clone(), Bytes::new()))
    }
}

#[cfg(test)]
mod tests {
    use types::testutil::*;

    use super::*;

    #[test]
    fn test_strip_split_hg_file_metadata() -> Result<()> {
        let key = key("foo/bar/baz", "1234");
        let data = Bytes::copy_from_slice(
            format!(
                "\x01\ncopy: {}\ncopyrev: {}\n\x01\nthis is a blob",
                key.path, key.hgid
            )
            .as_bytes(),
        );
        let (split_data, path) = strip_hg_file_metadata(&data)?;
        assert_eq!(split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, Some(key.clone()));

        let (blob, copy_from) = split_hg_file_metadata(&data)?;
        assert_eq!(blob, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(
            copy_from,
            Bytes::copy_from_slice(
                format!("\x01\ncopy: {}\ncopyrev: {}\n\x01\n", key.path, key.hgid).as_bytes(),
            )
        );

        let data = Bytes::from(&b"\x01\n\x01\nthis is a blob"[..]);
        let (split_data, path) = strip_hg_file_metadata(&data)?;
        assert_eq!(split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, None);

        let (blob, copy_from) = split_hg_file_metadata(&data)?;
        assert_eq!(blob, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(copy_from, &b"\x01\n\x01\n"[..]);

        let data = Bytes::from(&b"\x01\nthis is a blob"[..]);
        let (split_data, path) = strip_hg_file_metadata(&data)?;
        assert_eq!(split_data, data);
        assert_eq!(path, None);

        let (blob, copy_from) = split_hg_file_metadata(&data)?;
        assert_eq!(blob, data);
        assert_eq!(copy_from, Bytes::new());

        Ok(())
    }
}
