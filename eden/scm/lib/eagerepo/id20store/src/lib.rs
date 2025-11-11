/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Result;
use anyhow::ensure;
use format_util::strip_sha1_header;
use id20store_trait::Id20StoreExtension;
use minibytes::Bytes;
use parking_lot::RwLock;
use storemodel::SerializationFormat;
use zstore::Id20;
use zstore::Zstore;

/// Storage keyed by `Id20`. Intended to store file, tree, commit contents, and
/// be used by `EagerRepo` and `OnDiskCommits`. Wrapped by `Arc<RwLock>` for
/// easier sharing.
///
/// By default, SHA1 is verifiable, except for objects created by `virtual-repo`.
/// For HG this means `sorted([p1, p2])` and filelog rename metadata is included
/// in values. For Git this means `type size` is part of the prefix of the
/// stored blobs.
///
/// By default, backed by [`zstore::Zstore`], a pure content key-value store.
/// But users can extend the key-value interface using extensions.
#[derive(Clone)]
pub struct Id20Store {
    pub(crate) inner: Arc<RwLock<Zstore>>,
    format: SerializationFormat,
    ext: OnceLock<Arc<dyn Id20StoreExtension>>,
    pub(crate) extensions_path: PathBuf,
    pub(crate) extension_names: Arc<RwLock<BTreeSet<String>>>,
}

impl Id20Store {
    /// Open an [`Id20Store`] at the given directory.
    /// Create an empty store on demand.
    pub fn open(dir: &Path, format: SerializationFormat) -> Result<Self> {
        let inner = Zstore::open(dir)?;

        let extensions_path = dir.join("enabled-exts");
        let extension_names: BTreeSet<String> = {
            let content = match fs::read_to_string(&extensions_path) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e.into()),
            };
            content.lines().map(ToOwned::to_owned).collect()
        };

        let store = Self {
            inner: Arc::new(RwLock::new(inner)),
            format,
            ext: Default::default(),
            extensions_path,
            extension_names: Arc::new(RwLock::new(extension_names.clone())),
        };

        // Load extensions.
        for name in extension_names {
            let ext = factory::call_constructor::<_, Arc<dyn Id20StoreExtension>>(&(name, format))?;
            store.enable_extension(ext)?;
        }

        Ok(store)
    }

    /// Like `enable_extension`, but writes an on-disk file so the extension
    /// will also be enabled on the next `open`.
    ///
    /// Unlike `enable_extension`, the extension is identified by a string name.
    /// The extension provider should use [`factory::register_constructor`] to
    /// tell Id20Store how to convert the name to extension.
    pub fn enable_extension_permanently(&self, name: &'static str) -> Result<()> {
        // The ext name should be registered. Check it.
        let ext = factory::call_constructor::<_, Arc<dyn Id20StoreExtension>>(&(
            name.to_string(),
            self.format(),
        ))?;
        ensure!(
            ext.name() == name,
            "bug: extension name should match factory constructor name"
        );
        let mut names = self.extension_names.write();
        if !names.insert(name.to_string()) {
            // Already enabled.
            return Ok(());
        }
        atomicfile::atomic_write(&self.extensions_path, 0o660, false, |f| {
            let mut content = String::with_capacity(names.iter().map(|s| s.len() + 1).sum());
            for req in names.iter() {
                content.push_str(req);
                content.push('\n');
            }
            f.write_all(content.as_bytes())
        })?;
        self.enable_extension(ext)?;
        Ok(())
    }

    /// Extends the current `Id20Store` with an extension.
    fn enable_extension(&self, ext: Arc<dyn Id20StoreExtension>) -> anyhow::Result<()> {
        let got_ext = self.ext.get_or_init(|| ext.clone());
        ensure!(
            Arc::ptr_eq(got_ext, &ext),
            "bug: enable_extension called twice"
        );
        Ok(())
    }

    /// Flush changes to disk.
    pub fn flush(&self) -> Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    /// In git's case, the `data` should include the type and size header.
    pub fn add_sha1_blob(&self, data: &[u8], bases: &[Id20]) -> Result<Id20> {
        let mut inner = self.inner.write();
        Ok(inner.insert(data, bases)?)
    }

    /// Insert arbitrary blob with an `id`.
    /// This is usually used for hg's LFS data.
    pub fn add_arbitrary_blob(&self, id: Id20, data: &[u8]) -> Result<()> {
        let mut inner = self.inner.write();
        inner.insert_arbitrary(id, data, &[])?;
        Ok(())
    }

    /// Read SHA1 blob from zstore, including the prefixes.
    pub fn get_sha1_blob(&self, id: Id20) -> Result<Option<Bytes>> {
        if let Some(ext) = self.ext.get() {
            if let Some(blob) = ext.get_sha1_blob(id) {
                return Ok(Some(blob));
            }
        }
        let inner = self.inner.read();
        Ok(inner.get(id)?)
    }

    /// Get the current extension name.
    pub fn ext_name(&self) -> Option<&str> {
        self.ext.get().map(|e| e.name())
    }

    pub fn format(&self) -> SerializationFormat {
        self.format
    }

    /// Read the blob with its p1, p2 prefix removed.
    pub fn get_content(&self, id: Id20) -> Result<Option<Bytes>> {
        if let Some(ext) = self.ext.get() {
            if let Some(content) = ext.get_content(id) {
                return Ok(Some(content));
            }
        }
        // Special null case.
        if id.is_null() {
            return Ok(Some(Bytes::default()));
        }
        match self.get_sha1_blob(id)? {
            None => Ok(None),
            Some(data) => {
                let data = strip_sha1_header(&data, self.format())?;
                Ok(Some(data))
            }
        }
    }
}
