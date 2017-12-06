// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::str::{self, FromStr};

use errors::*;
use failure;
use mercurial_types::{BlobNode, MPath, NodeHash, Parents, NULL_HASH};
use mercurial_types::changeset::{Changeset, Time};

#[cfg(test)]
mod test;

// The `user` and `comments` fields are expected to be utf8 encoded, but
// some older commits might be corrupted. We handle them as pure binary here
// and higher levels can convert to utf8 as needed.
// See https://www.mercurial-scm.org/wiki/EncodingStrategy for details.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RevlogChangeset {
    parents: Parents,
    manifestid: NodeHash,
    user: Vec<u8>,
    time: Time,
    extra: Extra,
    files: Vec<MPath>,
    comments: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
struct Extra(BTreeMap<Vec<u8>, Vec<u8>>);

fn parseline<'a, I, F, T>(lines: &mut I, parse: F) -> Result<T>
where
    I: Iterator<Item = &'a [u8]>,
    F: Fn(&'a [u8]) -> Result<T>,
{
    match lines.next() {
        Some(s) => parse(s).map_err(Into::into),
        None => Err(failure::err_msg("premature end"))?,
    }
}

#[allow(dead_code)] // XXX TODO
fn escape<'a, S: IntoIterator<Item = &'a u8>>(s: S) -> Vec<u8> {
    let mut ret = Vec::new();

    for c in s.into_iter() {
        match *c {
            b'\0' => ret.extend_from_slice(&b"\\0"[..]),
            b'\n' => ret.extend_from_slice(&b"\\n"[..]),
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
            b'n' if quote => {
                quote = false;
                ret.push(b'\n');
            }
            b'0' if quote => {
                quote = false;
                ret.push(b'\0');
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

    pub fn generate<W: Write>(&self, out: &mut W) -> io::Result<()> {
        // assume BTreeMap is sorted enough
        let kv: Vec<_> = self.0
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
fn parsetimeextra<S: AsRef<[u8]>>(s: S) -> Result<(Time, Extra)> {
    let s = s.as_ref();
    let parts: Vec<_> = s.splitn(3, |c| *c == b' ').collect();

    if parts.len() < 2 {
        Err(failure::err_msg("not enough parts"))?;
    }
    let time: u64 = str::from_utf8(parts[0])?
        .parse::<u64>()
        .context("can't parse time")?;
    let tz: i32 = str::from_utf8(parts[1])?
        .parse::<i32>()
        .context("can't parse tz")?;

    let extras = Extra::from_slice(try_get(parts.as_ref(), 2))?;

    Ok((Time { time: time, tz: tz }, extras))
}

impl RevlogChangeset {
    pub fn new<T: AsRef<[u8]>>(node: BlobNode<T>) -> Result<Self> {
        Self::parse(node)
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
    fn parse<T: AsRef<[u8]>>(node: BlobNode<T>) -> Result<Self> {
        // This is awkward - we want to store the node in the resulting
        // RevlogChangeset but we need to borrow from it to parse its data. Set up a
        // partially initialized RevlogChangeset then fill it in as we go.
        let mut ret = Self {
            parents: *node.parents(),
            manifestid: NULL_HASH,
            user: Vec::new(),
            time: Time { time: 0, tz: 0 },
            extra: Extra(BTreeMap::new()),
            files: Vec::new(),
            comments: Vec::new(),
        };

        {
            let data = node.as_blob()
                .as_slice()
                .ok_or(failure::err_msg("node has no data"))?;
            let mut lines = data.split(|b| *b == b'\n');

            ret.manifestid = parseline(&mut lines, |l| NodeHash::from_str(str::from_utf8(l)?))
                .context("can't get hash")?;
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
        write!(out, "{}\n", self.manifestid)?;
        out.write_all(&self.user)?;
        out.write_all(b"\n")?;
        write!(out, "{} {} ", self.time.time, self.time.tz)?;
        self.extra.generate(out)?;
        write!(out, "\n")?;
        for f in &self.files {
            write!(out, "{}\n", f)?;
        }
        write!(out, "\n")?;
        out.write_all(&self.comments)?;

        Ok(())
    }

    pub fn get_node(&self) -> Result<BlobNode<Vec<u8>>> {
        let mut v = Vec::new();

        self.generate(&mut v)?;
        let (p1, p2) = self.parents.get_nodes();
        Ok(BlobNode::new(v, p1, p2))
    }
}

impl Changeset for RevlogChangeset {
    fn manifestid(&self) -> &NodeHash {
        &self.manifestid
    }

    fn user(&self) -> &[u8] {
        &self.user
    }

    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.extra.0
    }

    fn comments(&self) -> &[u8] {
        self.comments.as_ref()
    }

    fn files(&self) -> &[MPath] {
        self.files.as_ref()
    }

    fn time(&self) -> &Time {
        &self.time
    }

    fn parents(&self) -> &Parents {
        &self.parents
    }
}
