// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate blobstore;
extern crate bytes;
extern crate cloned;
extern crate context;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate libnfs_async;
extern crate mononoke_types;
#[cfg(test)]
extern crate quickcheck;
extern crate rand;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate smc;
extern crate twox_hash;

use std::fmt;
use std::hash::Hasher;
use std::io;
use std::path::{Path, PathBuf};

use bytes::{BigEndian, ByteOrder};
use cloned::cloned;
use failure::{format_err, Error};
use futures::future;
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use libnfs_async::{AsyncNfsContext, Mode, OFlag};
use rand::prelude::*;
use twox_hash::{XxHash, XxHash32};

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

// UID and GID we're using for file ownership and permissions checking.
const UID: u32 = 0;
const GID: u32 = 0;

const DIRMODE: Mode = Mode::S_IRWXU;
const FILEMODE: Mode = Mode::S_IRUSR;
const MAX_NAMELEN: usize = 232; // normal 255 name len with some space for prefix/suffix
const MAX_METADATA: u64 = 8 * 1024; // relatively small upper bound on metadata

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
struct GlusterBlobMetadata {
    /// original key
    key: String,
    /// xxhash64 of the contents
    xxhash64: Option<u64>,
}

pub struct Glusterblob {
    ctxt: AsyncNfsContext,
    host: String,
    export: String,
    basepath: PathBuf,
}

impl Glusterblob {
    pub fn with_smc(
        tier: impl Into<String>,
        export: impl Into<String>,
        basepath: impl Into<PathBuf>,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        let tier = tier.into();
        let export = export.into();
        let basepath = basepath.into();

        smc::get_available_services(&tier, false)
            .map(|services| {
                // Get all hosts for services
                let services: Vec<_> = services.iter().collect();
                services
                    .iter()
                    .filter(|svc| svc.is_production())
                    .map(|svc| svc.hostname.to_string())
                    .collect::<Vec<_>>()
            })
            .into_future()
            .and_then({
                cloned!(basepath, export);
                move |hosts| {
                    if hosts.is_empty() {
                        Err(format_err!(
                            "No available hosts in SMC for tier {}, {}:{}",
                            tier,
                            export,
                            basepath.display()
                        ))
                    } else {
                        Ok(hosts)
                    }
                }
            })
            .and_then(move |hosts| Self::with_hosts(hosts, export, basepath))
    }

    pub fn with_hosts(
        hosts: impl IntoIterator<Item = impl Into<String>>,
        export: impl Into<String>,
        basepath: impl Into<PathBuf>,
    ) -> impl Future<Item = Self, Error = Error> + Send + 'static {
        let export = export.into();
        let basepath = basepath.into();
        let hosts: Vec<String> = hosts.into_iter().map(Into::into).collect();

        let hosts = if hosts.is_empty() {
            Err(format_err!("No hosts specified"))
        } else {
            Ok(hosts)
        }.into_future();

        hosts.and_then(move |hosts| {
            let ctxts = hosts.into_iter().map({
                cloned!(export);
                move |host| AsyncNfsContext::mount(&host, &*export).map(|ctxt| (ctxt, host))
            });
            future::select_ok(ctxts)
                .map(|((ctxt, host), _)| (ctxt, host)) // don't care about unchosen hosts
                .and_then(|(ctxt, host)| ctxt.set_auth(UID, GID).map(move |()| (ctxt, host)))
                .from_err() // io::Error -> Error
                .map(move |(ctxt, host)| Glusterblob {
                    ctxt,
                    host,
                    export,
                    basepath,
                })
        })
    }

    pub fn get_hostname(&self) -> &str {
        &*self.host
    }

    pub fn get_export(&self) -> &str {
        &*self.export
    }

    pub fn get_basepath(&self) -> &Path {
        &*self.basepath
    }

    /// Return the path to a dir for a given key
    fn keydir(&self, key: &str) -> PathBuf {
        let hash = name_xxhash(key);
        let mut prefix = [0; 4];
        BigEndian::write_u32(&mut prefix, hash as u32);

        let mut path = self.basepath.clone();
        for p in &prefix {
            path.push(format!("{:02x}", p))
        }

        path
    }

    /// Return filename (not the whole path) for a given key
    fn keyfile(key: &str) -> PathBuf {
        PathBuf::from(format!("{}.data", keymangle(key)))
    }

    /// Return tmpfile name for writing a given key. This relies on (FB?) gluster's
    /// "rsync rename" hack, where a filename of the form ".<filename>.<someext>" are mapped
    /// to the same node as "<filename>".
    fn tmpfile(key: &str, ext: &str) -> PathBuf {
        PathBuf::from(format!(".{}.tmp{}{}", keymangle(key), ext, random::<u64>()))
    }

    /// Return the metadata file for a given key (named in a similar way to tmpfiles
    /// so they end up on the same node)
    fn metafile(key: &str) -> PathBuf {
        PathBuf::from(format!(".{}.meta", keymangle(key)))
    }

    /// Create the directory for a given key if it doesn't already exist
    fn create_keydir(&self, key: &str) -> impl Future<Item = PathBuf, Error = io::Error> {
        let path = self.keydir(key);

        // stat first to check if its there (don't worry about perms or even if its actually a dir)
        self.ctxt
            .stat64(path.clone())
            .then(missing_is_none)
            .and_then({
                cloned!(self.ctxt, path);
                move |found| match found {
                    Some(_) => Ok(path).into_future().left_future(),
                    None => ctxt.mkpath(path.clone(), DIRMODE)
                        .map(move |()| path)
                        .right_future(),
                }
            })
    }

    fn data_xxhash(data: &BlobstoreBytes) -> u64 {
        let mut hasher = XxHash::with_seed(0);
        hasher.write(data.as_bytes());
        hasher.finish()
    }
}

impl fmt::Debug for Glusterblob {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Glusterblob")
            .field("host", &self.host)
            .field("export", &self.host)
            .field("basepath", &self.basepath.display())
            .finish()
    }
}

impl Blobstore for Glusterblob {
    /// Fetch the value associated with `key`, or None if no value is present
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let path = self.keydir(&*key);
        let datapath = path.join(Self::keyfile(&*key));
        let metapath = path.join(Self::metafile(&*key));

        // Open path; if it doesn't exist then succeed with None, otherwise return the failure.
        // If it opens OK, then stat to get the size of the file, and try to read it in a single
        // read. Fail if its a short (or long) read.
        // TODO: do multiple reads to get whole file.
        let data = self.ctxt
            .open(datapath, OFlag::O_RDONLY)
            .then(missing_is_none)
            .and_then(|found| match found {
                Some(fh) => fh.fstat()
                    .map(move |st| (st.nfs_size, fh))
                    .and_then(|(sz, fh)| fh.read(sz).map(move |v| (v, sz)))
                    .and_then(|(vec, sz)| {
                        // TODO: multiple reads?
                        if vec.len() as u64 != sz {
                            Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("short read (got {} expected {})", vec.len(), sz),
                            ))
                        } else {
                            Ok(Some(BlobstoreBytes::from_bytes(vec)))
                        }
                    })
                    .right_future(),
                None => Ok(None).into_future().left_future(),
            });

        let meta = self.ctxt
            .open(metapath, OFlag::O_RDONLY)
            .then(missing_is_none)
            .and_then(|found| match found {
                None => Ok(None).into_future().left_future(),
                Some(fh) => fh.read(MAX_METADATA).map(Some).right_future(),
            })
            .and_then(|res| match res {
                Some(vec) => serde_json::from_slice::<GlusterBlobMetadata>(&*vec)
                    .map(Some)
                    .map_err(|err| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("Can't decode metadata: {}", err),
                        )
                    }),
                None => Ok(None),
            });

        data.join(meta)
            .and_then(move |(data, meta)| match (data, meta) {
                // Treat all partial cases as missing, since they're probably just an
                // incomplete put().
                (None, None) | (Some(_), None) | (None, Some(_)) => Ok(None),
                (Some(data), Some(meta)) => {
                    // Check xxhash if we have it
                    if let Some(xxhash) = meta.xxhash64 {
                        let hash = Self::data_xxhash(&data);
                        if hash != xxhash {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "key {}: xxhash mismatch: computed {:016x}, expected {:016x}",
                                    key, hash, xxhash,
                                ),
                            ));
                        }
                    };
                    Ok(Some(data))
                }
            })
            .from_err()
            .boxify()
    }

    /// Associate `value` with `key` for future gets; if `put` is called with different `value`s
    /// for the same key, the implementation may return any `value` it's been given in response
    /// to a `get` for that `key`.
    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.create_keydir(&*key)
            .and_then({
                cloned!(self.ctxt);
                move |path| {
                    let tmpfile = path.join(Self::tmpfile(&*key, "data"));
                    let file = path.join(Self::keyfile(&*key));
                    let tmpmeta = path.join(Self::tmpfile(&*key, "meta"));
                    let metafile = path.join(Self::metafile(&*key));

                    let meta = GlusterBlobMetadata {
                        key: key.clone(),
                        xxhash64: Some(Self::data_xxhash(&value)),
                    };
                    let metavec = serde_json::to_vec_pretty(&meta)
                        .expect("json serialization of metadata failed");

                    // Create, write to tmpfile, fsync and rename the data file
                    let data = ctxt.create(
                        tmpfile.clone(),
                        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY,
                        FILEMODE,
                    ).and_then(move |fh| {
                            let sz = value.len();
                            fh.write(value.as_bytes().to_vec())
                                .map(move |written| (fh, sz, written))
                        })
                        .and_then({
                            cloned!(key);
                            move |(fh, wanted, written)| {
                                if wanted != written {
                                    Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!(
                                            "key {}: short data write: wanted {} wrote {}",
                                            key, wanted, written
                                        ),
                                    )).into_future()
                                        .left_future()
                                } else {
                                    fh.fsync().right_future()
                                }
                            }
                        })
                        .and_then({
                            cloned!(ctxt);
                            move |()| ctxt.rename(tmpfile, file)
                        });

                    // Create, write to tmpfile, fsync and rename the metadata file
                    let meta = ctxt.create(
                        tmpmeta.clone(),
                        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY,
                        FILEMODE,
                    ).and_then(move |fh| {
                            let expected = metavec.len();
                            fh.write(metavec).map(move |wrote| (fh, wrote, expected))
                        })
                        .and_then({
                            cloned!(key);
                            move |(fh, wrote, expected)| {
                                if wrote != expected {
                                    Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!(
                                            "key {}: short write for metadata wrote {} expected {}",
                                            key, wrote, expected
                                        ),
                                    )).into_future()
                                        .left_future()
                                } else {
                                    fh.fsync().right_future()
                                }
                            }
                        })
                        .and_then(move |()| ctxt.rename(tmpmeta, metafile));

                    // Wait for everything to succeed
                    // XXX Clean up tmpfiles if there was a failure?
                    // XXX Clean up one file if other failed?
                    //     What if it races with a parallel put to the same key?
                    // XXX Look for existing files?
                    data.join(meta).map(|((), ())| ()).from_err().boxify()
                }
            })
            .from_err()
            .boxify()
    }

    /// Check that `get` will return a value for a given `key`, and not None. The provided
    /// implentation just calls `get`, and discards the return value; this can be overridden to
    /// avoid transferring data. In the absence of concurrent `put` calls, this must return
    /// `false` if `get` would return `None`, and `true` if `get` would return `Some(_)`.
    fn is_present(&self, _ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let path = self.keydir(&*key);
        let datapath = path.join(Self::keyfile(&*key));
        let metapath = path.join(Self::metafile(&*key));

        let check_data = self.ctxt.stat64(datapath).then(missing_is_none);
        let check_meta = self.ctxt.stat64(metapath).then(missing_is_none);

        check_data
            .join(check_meta)
            .map(|(data, meta)| data.is_some() && meta.is_some())
            .from_err()
            .boxify()
    }
}

/// Translate a "not found" error into None, any other success Some(x), and return
/// any other failure.
fn missing_is_none<T>(res: Result<T, io::Error>) -> Result<Option<T>, io::Error> {
    match res {
        Ok(v) => Ok(Some(v)),
        Err(ref err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn name_xxhash(key: &str) -> u32 {
    let mut hasher = XxHash32::with_seed(0);
    hasher.write(key.as_bytes());
    hasher.finish() as u32
}

/// Mangle a key to make it safe to use as a path - ie, remove / and . characters
/// (`/` for general Posix-safety, and `.` to avoid confusing rsync-hack names).
/// XXX Handle long pathnames? Truncate and use hash as disambiguator? (Not reversable, but
/// we still have the original key in metadata).
fn keymangle(key: &str) -> String {
    let mut ret = String::with_capacity(key.len());

    for c in key.chars() {
        match c {
            '/' => ret.push_str("#_"),
            '#' => ret.push_str("##"),
            '.' => ret.push_str("#@"),
            x => ret.push(x),
        }
    }

    const EXTRA: usize = 9; // how much we add to the truncated name
    if ret.len() >= MAX_NAMELEN - EXTRA {
        // XXX truncate in the middle of a character? Or change everything to bytes?
        // Hack it for now, but we still need to be careful because `str`'s slice operator
        // will panic if it splits a character.
        let hash = name_xxhash(key);
        ret = format!(
            "{}:{:08x}",
            String::from_utf8_lossy(&ret.as_bytes()[..MAX_NAMELEN - EXTRA]),
            hash
        );
    }

    ret
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    // OK for testing, but not useful in general as it can't demangle hash-truncated keys
    fn keydemangle(mangled: &str) -> String {
        let mut ret = String::with_capacity(mangled.len());

        let mut quoted = false;
        for c in mangled.chars() {
            match c {
                '_' if quoted => ret.push('/'),
                '#' if quoted => ret.push('#'),
                '@' if quoted => ret.push('.'),
                '#' => {
                    quoted = true;
                    continue;
                }
                x => ret.push(x),
            }
            quoted = false;
        }

        ret
    }

    #[test]
    fn mangle_noop() {
        assert_eq!(
            keymangle("normal-string-with-normal-things"),
            "normal-string-with-normal-things"
        )
    }

    #[test]
    fn mangle_path() {
        assert_eq!(keymangle("this/is/a/path"), "this#_is#_a#_path")
    }

    #[test]
    fn mangle_hash() {
        assert_eq!(keymangle("#this"), "##this")
    }

    #[test]
    fn mangle_dot() {
        assert_eq!(keymangle("this.that"), "this#@that")
    }

    #[test]
    fn mangle_longname() {
        let longname = concat!(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        );
        let truncated = concat!(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "xxxxxxx:3f68667b"
        );
        let mangled = keymangle(longname);

        assert!(
            mangled.len() <= MAX_NAMELEN,
            "mangled.len = {}, max {}",
            mangled.len(),
            MAX_NAMELEN
        );
        assert_eq!(mangled, truncated);
    }

    #[test]
    fn mangle_longname_expanded() {
        let longname = concat!(
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
            "........................................................................",
        );
        let truncated = concat!(
            "#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@",
            "#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@",
            "#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@#@",
            "#@#@#@#:89c18039"
        );
        let mangled = keymangle(longname);

        assert!(
            mangled.len() <= MAX_NAMELEN,
            "mangled.len = {}, max {}",
            mangled.len(),
            MAX_NAMELEN
        );
        assert_eq!(mangled, truncated);
    }

    #[test]
    fn mangle_roundtrip() {
        let tests = &[
            "normal-string-with-normal-things",
            "this/is/a/path",
            "/abspath",
            "redundant//path",
            "trailingpath/",
            "./../.././//.../",
            "#/.#.//.#/.?>3.3/..#",
            "this#that",
            "#this",
            "this.that",
        ];

        for t in tests {
            assert_eq!(*t, &*keydemangle(&keymangle(t)))
        }
    }

    quickcheck! {
        fn qc_mangle_roundtrip(s: String) -> bool {
            s == keydemangle(&*keymangle(&*s))
        }
    }
}
