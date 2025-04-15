/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! See https://repo.mercurial-scm.org/hg/rev/10519e4cbd02 for the "hg file metadata" format.

use std::str;
use std::str::FromStr;

use anyhow::Result;
use anyhow::bail;
use minibytes::Bytes;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::SerializationFormat;

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
///
/// Git objects do not include this extra information, so this method no-ops for those objects.
pub fn strip_file_metadata(
    data: &Bytes,
    format: SerializationFormat,
) -> Result<(Bytes, Option<Key>)> {
    match format {
        SerializationFormat::Hg => {
            let (blob, copy_from) = split_hg_file_metadata(data);
            Ok((blob, parse_copy_from_hg_file_metadata(copy_from.as_ref())?))
        }
        SerializationFormat::Git => Ok((data.clone(), None)),
    }
}

pub fn parse_copy_from_hg_file_metadata(data: &[u8]) -> Result<Option<Key>> {
    if data.is_empty() {
        return Ok(None);
    }

    let data = &data[2..data.len() - 2];
    let mut path = None;
    let mut hgid = None;

    for line in data.split(|c| c == &b'\n') {
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

    match (path, hgid) {
        (None, Some(_)) => bail!("missing 'copyrev' metadata"),
        (Some(_), None) => bail!("missing 'copy' metadata"),

        (None, None) => Ok(None),
        (Some(path), Some(hgid)) => Ok(Some(Key::new(path, hgid))),
    }
}

pub fn split_file_metadata(data: &Bytes, format: SerializationFormat) -> (Bytes, Option<Bytes>) {
    match format {
        SerializationFormat::Hg => {
            let (content, header) = split_hg_file_metadata(data);
            (content, Some(header))
        }
        SerializationFormat::Git => (data.clone(), None),
    }
}

pub fn split_hg_file_metadata(data: &Bytes) -> (Bytes, Bytes) {
    let slice = data.as_ref();
    if !slice.starts_with(b"\x01\n") {
        return (data.clone(), Bytes::new());
    }
    let slice = &slice[2..];
    if let Some(pos) = slice.windows(2).position(|needle| needle == b"\x01\n") {
        let split_pos = 2 + pos + 2;
        (data.slice(split_pos..), data.slice(..split_pos))
    } else {
        (data.clone(), Bytes::new())
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

        // Hg format should strip the copy data from the blob
        let (hg_split_data, path) = strip_file_metadata(&data, SerializationFormat::Hg)?;
        assert_eq!(hg_split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, Some(key.clone()));

        // Git format should no-op; copy metadata isn't interpreted for git objects
        let (git_split_data, path) = strip_file_metadata(&data, SerializationFormat::Git)?;
        assert_eq!(git_split_data, data);
        assert_eq!(path, None);

        let (blob, copy_from) = split_hg_file_metadata(&data);
        assert_eq!(blob, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(
            copy_from,
            Bytes::copy_from_slice(
                format!("\x01\ncopy: {}\ncopyrev: {}\n\x01\n", key.path, key.hgid).as_bytes(),
            )
        );

        let data = Bytes::from(&b"\x01\n\x01\nthis is a blob"[..]);
        let (hg_split_data, path) = strip_file_metadata(&data, SerializationFormat::Hg)?;
        assert_eq!(hg_split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, None);

        // Same as above, Git format should no-op
        let data = Bytes::from(&b"\x01\n\x01\nthis is a blob"[..]);
        let (git_split_data, path) = strip_file_metadata(&data, SerializationFormat::Git)?;
        assert_eq!(git_split_data, data);
        assert_eq!(path, None);

        let (blob, copy_from) = split_hg_file_metadata(&data);
        assert_eq!(blob, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(copy_from, &b"\x01\n\x01\n"[..]);

        // Git and hg behave the same in this case
        let data = Bytes::from(&b"\x01\nthis is a blob"[..]);
        let (hg_split_data, hg_path) = strip_file_metadata(&data, SerializationFormat::Hg)?;
        let (git_split_data, git_path) = strip_file_metadata(&data, SerializationFormat::Git)?;
        assert_eq!(hg_split_data, data);
        assert_eq!(git_split_data, hg_split_data);
        assert_eq!(hg_path, None);
        assert_eq!(git_path, hg_path);

        let (blob, copy_from) = split_hg_file_metadata(&data);
        assert_eq!(blob, data);
        assert_eq!(copy_from, Bytes::new());

        Ok(())
    }
}
