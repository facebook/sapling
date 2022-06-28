/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fs::create_dir_all;
use std::ops::RangeBounds;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use percent_encoding::percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::CONTROLS;

use blobstore::Blobstore;
use blobstore::BlobstoreEnumerationData;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstoreKeyParam;
use blobstore::BlobstoreKeySource;
use blobstore::BlobstoreMetadata;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use tempfile::NamedTempFile;
use tempfile::PersistError;
use tokio::fs::hard_link;
use tokio::fs::remove_file;
use tokio::fs::File;
use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use walkdir::WalkDir;

const PREFIX: &str = "blob";
const PREFIX_HYPHEN: &str = "blob-";
// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
// https://url.spec.whatwg.org/#path-percent-encode-set
const PATH: &AsciiSet = &FRAGMENT.add(b'#').add(b'?').add(b'{').add(b'}');

#[derive(Debug, Clone)]
pub struct Fileblob {
    base: PathBuf,
    put_behaviour: PutBehaviour,
}

impl Fileblob {
    pub fn open<P: AsRef<Path>>(base: P, put_behaviour: PutBehaviour) -> Result<Self> {
        let base = base.as_ref();

        if !base.is_dir() {
            bail!("Base {:?} doesn't exist or is not directory", base);
        }

        Ok(Self {
            base: base.to_owned(),
            put_behaviour,
        })
    }

    pub fn create<P: AsRef<Path>>(base: P, put_behaviour: PutBehaviour) -> Result<Self> {
        let base = base.as_ref();
        create_dir_all(base)?;
        Self::open(base, put_behaviour)
    }

    fn path(&self, key: &str) -> PathBuf {
        let key = percent_encode(key.as_bytes(), PATH);
        self.base.join(format!("{}-{}", PREFIX, key))
    }

    /// Stripping the prepended prefix (if its exists) before returning
    /// keys back to the caller. Safe to call with or without the prefix.
    fn strip_file_prefix<'a>(&self, key: &'a str) -> &'a str {
        match key.strip_prefix(PREFIX_HYPHEN) {
            // Remove the prefix if present and return the remaining
            // slice.
            Some(key_without_prefix) => key_without_prefix,
            // If not present, return the key as-is.
            None => key,
        }
    }
}

impl std::fmt::Display for Fileblob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fileblob")
    }
}

async fn ctime(file: &File) -> Option<i64> {
    let meta = file.metadata().await.ok()?;
    let ctime = meta.modified().ok()?;
    let ctime_dur = ctime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    i64::try_from(ctime_dur.as_secs()).ok()
}

#[async_trait]
impl BlobstorePutOps for Fileblob {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        let p = self.path(&key);
        // block_in_place on tempfile would be ideal here, but it interacts
        // badly with tokio_compat
        let tempfile = NamedTempFile::new_in(&self.base)?;
        let new_file = tempfile.as_file().try_clone()?;
        let mut tokio_file = File::from_std(new_file);
        tokio_file.write_all(value.as_bytes().as_ref()).await?;
        tokio_file.flush().await?;
        tokio_file.sync_all().await?;
        let status = match put_behaviour {
            PutBehaviour::Overwrite => {
                tempfile.persist(&p)?;
                OverwriteStatus::NotChecked
            }
            PutBehaviour::IfAbsent | PutBehaviour::OverwriteAndLog => {
                let temp_path = tempfile.path().to_owned();
                match tempfile.persist_noclobber(&p) {
                    Ok(_) => OverwriteStatus::New,
                    // Key already existed
                    Err(PersistError { file: f, error: e })
                        if f.path() == temp_path
                            && e.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        if put_behaviour.should_overwrite() {
                            f.persist(&p)?;
                            OverwriteStatus::Overwrote
                        } else {
                            OverwriteStatus::Prevented
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };

        Ok(status)
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_explicit(ctx, key, value, self.put_behaviour).await
    }
}

#[async_trait]
impl Blobstore for Fileblob {
    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let p = self.path(key);

        let ret = match File::open(&p).await {
            Err(ref r) if r.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
            Ok(mut f) => {
                let mut v = Vec::new();
                f.read_to_end(&mut v).await?;

                Some(BlobstoreGetData::new(
                    BlobstoreMetadata::new(ctime(&f).await, None),
                    BlobstoreBytes::from_bytes(v),
                ))
            }
        };
        Ok(ret)
    }

    async fn is_present<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let p = self.path(key);

        let present = match File::open(&p).await {
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
            Ok(_) => true,
        };
        Ok(if present {
            BlobstoreIsPresent::Present
        } else {
            BlobstoreIsPresent::Absent
        })
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }

    // This uses hardlink semantics as the production blobstores also have hardlink like semantics
    // (i.e. you can't discover a canonical link source when loading by the target)
    async fn copy<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        // from std::fs::hard_link: The dst path will be a link pointing to the src path
        let src_path = self.path(old_key);
        let dst_path = self.path(&new_key);
        // hard_link will fail if dst_path exists. Race it in a task of its own
        Ok(tokio::task::spawn(async move {
            let _ = remove_file(&dst_path).await;
            hard_link(src_path, dst_path).await
        })
        .await??)
    }
}

#[async_trait]
impl BlobstoreUnlinkOps for Fileblob {
    async fn unlink<'a>(&'a self, _ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        let path = self.path(key);
        Ok(remove_file(path).await?)
    }
}

#[async_trait]
impl BlobstoreKeySource for Fileblob {
    async fn enumerate<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        range: &'a BlobstoreKeyParam,
    ) -> Result<BlobstoreEnumerationData> {
        match range {
            BlobstoreKeyParam::Start(ref range) => {
                let mut enum_data = BlobstoreEnumerationData {
                    keys: HashSet::new(),
                    next_token: None,
                };
                WalkDir::new(&self.base)
                    .into_iter()
                    .filter_map(|v| v.ok())
                    .for_each(|entry| {
                        // Need the filename not the directory, since the directory
                        // structure is not exposed to the caller.
                        let entry = entry.file_name().to_str();
                        if let Some(data) = entry {
                            let key = self.strip_file_prefix(data).to_string();
                            if range.contains(&key) {
                                enum_data.keys.insert(key);
                            }
                        }
                    });
                Ok(enum_data)
            }
            _ => Err(format_err!("Fileblob does not support token, only ranges")),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use fbinit::FacebookInit;

    #[fbinit::test]
    async fn test_persist_error(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let blob = Fileblob {
            base: PathBuf::from("/mononoke/fileblob/test/path/should/not/exist"),
            put_behaviour: PutBehaviour::IfAbsent,
        };

        let ret = blob
            .put(&ctx, "key".into(), BlobstoreBytes::from_bytes("value"))
            .await;

        assert!(ret.is_err());

        Ok(())
    }
}
