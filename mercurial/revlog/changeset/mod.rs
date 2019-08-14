// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::bail_msg;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::str::{self, FromStr};

use crate::errors::*;
use bytes::Bytes;
use mercurial_types::{
    HgBlob, HgBlobNode, HgChangesetEnvelope, HgChangesetId, HgManifestId, HgNodeHash, HgParents,
    MPath, NULL_HASH,
};
use mononoke_types::DateTime;

#[cfg(test)]
mod test;

// The `user` and `comments` fields are expected to be utf8 encoded, but
// some older commits might be corrupted. We handle them as pure binary here
// and higher levels can convert to utf8 as needed.
// See https://www.mercurial-scm.org/wiki/EncodingStrategy for details.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RevlogChangeset {
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub manifestid: HgManifestId,
    pub user: Vec<u8>,
    pub time: DateTime,
    pub extra: Extra,
    pub files: Vec<MPath>,
    pub comments: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct Extra(BTreeMap<Vec<u8>, Vec<u8>>);

impl Extra {
    pub fn as_ref(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.0
    }
}

fn parseline<'a, I, F, T>(lines: &mut I, parse: F) -> Result<T>
where
    I: Iterator<Item = &'a [u8]>,
    F: Fn(&'a [u8]) -> Result<T>,
{
    match lines.next() {
        Some(s) => parse(s).map_err(Into::into),
        None => bail_msg!("premature end"),
    }
}

fn escape<'a, S: IntoIterator<Item = &'a u8>>(s: S) -> Vec<u8> {
    let mut ret = Vec::new();

    for c in s.into_iter() {
        match *c {
            b'\0' => ret.extend_from_slice(&b"\\0"[..]),
            b'\n' => ret.extend_from_slice(&b"\\n"[..]),
            b'\r' => ret.extend_from_slice(&b"\\r"[..]),
            b'\\' => ret.extend_from_slice(&b"\\\\"[..]),
            c => ret.push(c),
        }
    }

    ret
}

fn unescape<'a, S: IntoIterator<Item = &'a u8>>(s: S) -> Vec<u8> {
    let mut ret = Vec::new();
    let mut quote = false;

    for c in s.into_iter() {
        match *c {
            b'0' if quote => {
                quote = false;
                ret.push(b'\0');
            }
            b'n' if quote => {
                quote = false;
                ret.push(b'\n');
            }
            b'r' if quote => {
                quote = false;
                ret.push(b'\r');
            }
            b'\\' if quote => {
                quote = false;
                ret.push(b'\\');
            }
            c if quote => {
                quote = false;
                ret.push(b'\\');
                ret.push(c)
            }
            b'\\' => {
                assert!(!quote);
                quote = true;
            }
            c => {
                quote = false;
                ret.push(c);
            }
        }
    }

    ret
}

impl Extra {
    pub fn new(extra: BTreeMap<Vec<u8>, Vec<u8>>) -> Self {
        Extra(extra)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn from_slice<S: AsRef<[u8]>>(s: Option<S>) -> Result<Extra> {
        let mut ret = BTreeMap::new();

        if let Some(s) = s {
            let s = s.as_ref();

            for kv in s.split(|c| *c == b'\0') {
                let kv: Vec<_> = kv.splitn(2, |c| *c == b':').collect();
                if kv.len() == 2 {
                    ret.insert(unescape(kv[0]), unescape(kv[1]));
                }
            }
        }

        Ok(Extra(ret))
    }
}

fn try_get<T>(v: &[T], idx: usize) -> Option<&T> {
    let v = v.as_ref();
    if idx < v.len() {
        Some(&v[idx])
    } else {
        None
    }
}

// Time has the format: time tz extra\n
// "date (time is int or float, timezone is int)"
//     - in what units? time is seconds from epoch?
//     - what's TZ? seconds offset from UTC?
//
// Extra is key:value, \0 separated, with \\, \0, \n escaped
fn parsetimeextra<S: AsRef<[u8]>>(s: S) -> Result<(DateTime, Extra)> {
    let s = s.as_ref();
    let parts: Vec<_> = s.splitn(3, |c| *c == b' ').collect();

    if parts.len() < 2 {
        bail_msg!("not enough parts");
    }
    let time: i64 = str::from_utf8(parts[0])?
        .parse::<i64>()
        .context("can't parse time")?;
    let tz: i32 = str::from_utf8(parts[1])?
        .parse::<i32>()
        .context("can't parse tz")?;

    let extras = Extra::from_slice(try_get(parts.as_ref(), 2))?;

    Ok((DateTime::from_timestamp(time, tz)?, extras))
}

impl RevlogChangeset {
    pub fn new_from_parts(
        // XXX replace parents with p1 and p2
        parents: HgParents,
        manifestid: HgManifestId,
        user: Vec<u8>,
        time: DateTime,
        extra: BTreeMap<Vec<u8>, Vec<u8>>,
        files: Vec<MPath>,
        comments: Vec<u8>,
    ) -> Self {
        let (p1, p2) = parents.get_nodes();
        Self {
            p1,
            p2,
            manifestid,
            user,
            time,
            extra: Extra(extra),
            files,
            comments,
        }
    }

    pub fn new(node: HgBlobNode) -> Result<Self> {
        let (p1, p2) = node.parents().get_nodes();
        Self::parse(node.as_blob().clone(), p1, p2)
    }

    pub fn from_envelope(envelope: HgChangesetEnvelope) -> Result<Self> {
        let envelope = envelope.into_mut();
        Self::parse(
            envelope.contents.into(),
            envelope.p1.map(HgChangesetId::into_nodehash),
            envelope.p2.map(HgChangesetId::into_nodehash),
        )
    }

    pub fn new_null() -> Self {
        Self {
            p1: None,
            p2: None,
            manifestid: HgManifestId::new(NULL_HASH),
            user: Vec::new(),
            time: DateTime::from_timestamp(0, 0).expect("this is a valid DateTime"),
            extra: Extra(BTreeMap::new()),
            files: Vec::new(),
            comments: Vec::new(),
        }
    }

    // format used:
    // nodeid\n        : manifest node in ascii
    // user\n          : user, no \n or \r allowed
    // time tz extra\n : date (time is int or float, timezone is int)
    //                 : extra is metadata, encoded and separated by '\0'
    //                 : older versions ignore it
    // files\n\n       : files modified by the cset, no \n or \r allowed
    // (.*)            : comment (free text, ideally utf-8)
    //
    // changelog v0 doesn't use extra
    //
    // XXX Any constraints on/syntax of "user"?
    // XXX time units? tz meaning?
    // XXX Files sorted? No escaping?
    // XXX "extra" - how sorted? What encoding?
    // XXX "comment" - line endings normalized at all?
    fn parse(blob: HgBlob, p1: Option<HgNodeHash>, p2: Option<HgNodeHash>) -> Result<Self> {
        // This is awkward - we want to store the node in the resulting
        // RevlogChangeset but we need to borrow from it to parse its data. Set up a
        // partially initialized RevlogChangeset then fill it in as we go.
        let mut ret = Self {
            p1,
            p2,
            manifestid: HgManifestId::new(NULL_HASH),
            user: Vec::new(),
            time: DateTime::from_timestamp(0, 0).expect("this is a valid DateTime"),
            extra: Extra(BTreeMap::new()),
            files: Vec::new(),
            comments: Vec::new(),
        };

        {
            let data = blob.as_slice();
            let mut lines = data.split(|b| *b == b'\n');

            let nodehash = parseline(&mut lines, |l| HgNodeHash::from_str(str::from_utf8(l)?))
                .context("can't get hash")?;
            ret.manifestid = HgManifestId::new(nodehash);
            ret.user =
                parseline(&mut lines, |u| Ok::<_, Error>(u.to_vec())).context("can't get user")?;
            let (time, extra) =
                parseline(&mut lines, parsetimeextra).context("can't get time/extra")?;

            ret.time = time;
            ret.extra = extra;

            let mut files = Vec::new();
            let mut comments = Vec::new();

            // List of files followed by the comments. The file list is one entry
            // per line, with a blank line delimiting the end. The comments are a single
            // binary blob with no internal structure, but we've already split it on '\n'
            // bounaries, so we can glue it back together to re-create the original content.
            //
            // XXX: We assume the comment is utf-8. Is this a good assumption?
            let mut dofiles = true;
            for line in lines {
                if dofiles {
                    if line.len() == 0 {
                        dofiles = false;
                        continue;
                    }
                    files.push(MPath::new(line).context("invalid path in changelog")?)
                } else {
                    comments.push(line);
                }
            }

            ret.files = files;
            ret.comments = comments.join(&b'\n');
        }

        Ok(ret)
    }

    /// Generate a serialized changeset. This is the counterpart to parse, and generates
    /// in the same format as Mercurial. It should be bit-for-bit identical in fact.
    pub fn generate<W: Write>(&self, out: &mut W) -> Result<()> {
        serialize_cs(self, out)
    }

    pub fn get_node(&self) -> Result<HgBlobNode> {
        let mut v = Vec::new();
        self.generate(&mut v)?;
        Ok(HgBlobNode::new(Bytes::from(v), self.p1(), self.p2()))
    }

    pub fn manifestid(&self) -> HgManifestId {
        self.manifestid
    }

    pub fn user(&self) -> &[u8] {
        &self.user
    }

    pub fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.extra.0
    }

    pub fn comments(&self) -> &[u8] {
        self.comments.as_ref()
    }

    pub fn files(&self) -> &[MPath] {
        self.files.as_ref()
    }

    pub fn time(&self) -> &DateTime {
        &self.time
    }

    #[inline]
    pub fn parents(&self) -> HgParents {
        HgParents::new(self.p1(), self.p2())
    }

    #[inline]
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.p1
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.p2
    }
}

/// Generate a serialized changeset. This is the counterpart to parse, and generates
/// in the same format as Mercurial. It should be bit-for-bit identical in fact.
pub fn serialize_cs<W: Write>(cs: &RevlogChangeset, out: &mut W) -> Result<()> {
    write!(out, "{}\n", cs.manifestid().into_nodehash())?;
    out.write_all(cs.user())?;
    out.write_all(b"\n")?;
    write!(
        out,
        "{} {}",
        cs.time().timestamp_secs(),
        cs.time().tz_offset_secs()
    )?;

    if !cs.extra().is_empty() {
        write!(out, " ")?;
        serialize_extras(&cs.extra, out)?;
    }

    write!(out, "\n")?;
    for f in cs.files() {
        write!(out, "{}\n", f)?;
    }
    write!(out, "\n")?;
    out.write_all(&cs.comments())?;

    Ok(())
}

pub fn serialize_extras<W: Write>(extras: &Extra, out: &mut W) -> io::Result<()> {
    // assume BTreeMap is sorted enough
    let kv: Vec<_> = extras
        .0
        .iter()
        .map(|(k, v)| {
            let mut vec = Vec::new();
            vec.extend_from_slice(k);
            vec.push(b':');
            vec.extend_from_slice(v);
            escape(&vec)
        })
        .collect();
    out.write_all(kv.join(&b'\0').as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn escape_roundtrip(input: Vec<u8>) -> bool {
            let result = escape(input.iter());
            unescape(result.iter()) == input
        }
    }

    #[test]
    fn unescape_example_roundtrip() {
        let input = b"\x0c\\r\x90\x0c\x01\\n";
        let result = unescape(input.iter());
        assert_eq!(escape(result.iter()), input);
    }
}
