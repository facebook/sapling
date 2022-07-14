/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Root manifest, tree nodes

use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use manifest::Entry;
use manifest::Manifest;
use sorted_vector_map::SortedVectorMap;
use std::str;

use super::errors::ErrorKind;
use crate::nodehash::HgNodeHash;
use crate::nodehash::NULL_HASH;
use crate::FileType;
use crate::HgBlob;
use crate::HgFileNodeId;
use crate::HgManifestEnvelope;
use crate::HgManifestId;
use crate::HgParents;
use crate::MPathElement;
use crate::Type;

#[derive(Debug, Eq, PartialEq)]
pub struct ManifestContent {
    pub files: SortedVectorMap<MPathElement, Entry<HgManifestId, (FileType, HgFileNodeId)>>,
}

impl ManifestContent {
    pub fn new_empty() -> Self {
        Self {
            files: SortedVectorMap::new(),
        }
    }

    // Each manifest revision contains a list of the file revisions in each changeset, in the form:
    //
    // <filename>\0<hex file revision id>[<flags>]\n
    //
    // Source: mercurial/parsers.c:parse_manifest()

    //
    // NB: filenames are sequences of non-zero bytes, not strings
    fn parse_impl(
        data: &[u8],
    ) -> Result<SortedVectorMap<MPathElement, Entry<HgManifestId, (FileType, HgFileNodeId)>>> {
        let lines = data.split(|b| *b == b'\n');
        let mut files = match lines.size_hint() {
            // Split returns it count in the high size hint
            (_, Some(high)) => SortedVectorMap::with_capacity(high),
            (_, None) => SortedVectorMap::new(),
        };

        for line in lines {
            if line.is_empty() {
                break;
            }

            let (name, rest) = match find(line, &0) {
                None => bail!("Malformed entry: no \\0"),
                Some(nil) => {
                    let (name, rest) = line.split_at(nil);
                    if let Some((_, hash)) = rest.split_first() {
                        (name, hash)
                    } else {
                        bail!("Malformed entry: no hash");
                    }
                }
            };

            let path = MPathElement::new(name.to_vec()).context("invalid path in manifest")?;
            let entry_id = parse_hg_entry(rest)?;

            files.insert(path, entry_id);
        }

        Ok(files)
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        Ok(Self {
            files: Self::parse_impl(data)?,
        })
    }
}

pub async fn fetch_raw_manifest_bytes<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    manifest_id: HgManifestId,
) -> Result<HgBlob> {
    let envelope = fetch_manifest_envelope(ctx, blobstore, manifest_id).await?;
    let envelope = envelope.into_mut();
    Ok(HgBlob::from(envelope.contents))
}

pub async fn fetch_manifest_envelope<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    manifest_id: HgManifestId,
) -> Result<HgManifestEnvelope> {
    let envelope = fetch_manifest_envelope_opt(ctx, blobstore, manifest_id).await?;
    Ok(envelope
        .ok_or_else(move || ErrorKind::HgContentMissing(manifest_id.into_nodehash(), Type::Tree))?)
}

/// Like `fetch_manifest_envelope`, but returns None if the manifest wasn't found.
pub async fn fetch_manifest_envelope_opt<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    node_id: HgManifestId,
) -> Result<Option<HgManifestEnvelope>> {
    let blobstore_key = node_id.blobstore_key();
    let bytes = blobstore
        .get(ctx, &blobstore_key)
        .await
        .context("While fetching manifest envelope blob")?;
    (|| {
        let blobstore_bytes = match bytes {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        let envelope = HgManifestEnvelope::from_blob(blobstore_bytes.into())?;
        if node_id.into_nodehash() != envelope.node_id() {
            bail!(
                "Manifest ID mismatch (requested: {}, got: {})",
                node_id,
                envelope.node_id()
            );
        }
        Ok(Some(envelope))
    })()
    .context(ErrorKind::ManifestDeserializeFailed(blobstore_key))
}

#[derive(Debug)]
pub struct HgBlobManifest {
    node_id: HgNodeHash,
    p1: Option<HgNodeHash>,
    p2: Option<HgNodeHash>,
    // See the documentation in mercurial_types/if/mercurial.thrift for why this exists.
    computed_node_id: HgNodeHash,
    content: ManifestContent,
}

impl HgBlobManifest {
    pub async fn load<B: Blobstore>(
        ctx: &CoreContext,
        blobstore: &B,
        manifestid: HgManifestId,
    ) -> Result<Option<Self>> {
        if manifestid.clone().into_nodehash() == NULL_HASH {
            Ok(Some(HgBlobManifest {
                node_id: NULL_HASH,
                p1: None,
                p2: None,
                computed_node_id: NULL_HASH,
                content: ManifestContent::new_empty(),
            }))
        } else {
            async {
                let envelope = fetch_manifest_envelope_opt(ctx, blobstore, manifestid).await?;
                match envelope {
                    Some(envelope) => Ok(Some(Self::parse(envelope)?)),
                    None => Result::<_>::Ok(None),
                }
            }
            .await
            .context(format!(
                "When loading manifest {} from blobstore",
                manifestid
            ))
        }
    }

    pub fn parse(envelope: HgManifestEnvelope) -> Result<Self> {
        let envelope = envelope.into_mut();
        let content = ManifestContent::parse(envelope.contents.as_ref()).with_context(|| {
            format!(
                "while parsing contents for manifest ID {}",
                envelope.node_id
            )
        })?;
        Ok(HgBlobManifest {
            node_id: envelope.node_id,
            p1: envelope.p1,
            p2: envelope.p2,
            computed_node_id: envelope.computed_node_id,
            content,
        })
    }

    #[inline]
    pub fn node_id(&self) -> HgNodeHash {
        self.node_id
    }

    #[inline]
    pub fn p1(&self) -> Option<HgNodeHash> {
        self.p1
    }

    #[inline]
    pub fn p2(&self) -> Option<HgNodeHash> {
        self.p2
    }

    #[inline]
    pub fn hg_parents(&self) -> HgParents {
        HgParents::new(self.p1, self.p2)
    }

    #[inline]
    pub fn computed_node_id(&self) -> HgNodeHash {
        self.computed_node_id
    }

    pub fn content(&self) -> &ManifestContent {
        &self.content
    }
}

#[async_trait]
impl Loadable for HgManifestId {
    type Value = HgBlobManifest;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let id = *self;
        HgBlobManifest::load(ctx, blobstore, id)
            .await?
            .ok_or_else(|| LoadableError::Missing(id.blobstore_key()))
    }
}

impl Manifest for HgBlobManifest {
    type TreeId = HgManifestId;
    type LeafId = (FileType, HgFileNodeId);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.content.files.get(name).copied()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let iter = self.content.files.clone().into_iter();
        Box::new(iter)
    }
}

fn parse_hg_entry(data: &[u8]) -> Result<Entry<HgManifestId, (FileType, HgFileNodeId)>> {
    ensure!(data.len() >= 40, "hash too small: {:?}", data);

    let (hash, flags) = data.split_at(40);
    let hash = str::from_utf8(hash)
        .map_err(Error::from)
        .and_then(|hash| hash.parse::<HgNodeHash>())
        .with_context(|| format!("malformed hash: {:?}", hash))?;
    ensure!(flags.len() <= 1, "More than 1 flag: {:?}", flags);

    let hg_entry_id = if flags.is_empty() {
        Entry::Leaf((FileType::Regular, HgFileNodeId::new(hash)))
    } else {
        match flags[0] {
            b'l' => Entry::Leaf((FileType::Symlink, HgFileNodeId::new(hash))),
            b'x' => Entry::Leaf((FileType::Executable, HgFileNodeId::new(hash))),
            b't' => Entry::Tree(HgManifestId::new(hash)),
            unk => bail!("Unknown flag {}", unk),
        }
    };

    Ok(hg_entry_id)
}

fn find<T>(haystack: &[T], needle: &T) -> Option<usize>
where
    T: PartialEq,
{
    haystack.iter().position(|e| e == needle)
}
