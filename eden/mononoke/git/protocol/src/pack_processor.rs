/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use data::entry::Header::RefDelta;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use git_types::fetch_git_object_bytes;
use git_types::GitIdentifier;
use git_types::HeaderState;
use gix_features::progress::Discard;
use gix_hash::Kind;
use gix_hash::ObjectId;
use gix_object::ObjectRef;
use gix_pack::cache::Never;
use gix_pack::data;
use gix_pack::data::decode::entry::ResolvedBase;
use gix_pack::data::input;
use gix_pack::data::File;
use mononoke_types::hash::GitSha1;
use repo_blobstore::RepoBlobstore;
use tempfile::Builder;

type ObjectMap = HashMap<ObjectId, (Bytes, gix_object::Kind)>;

fn into_data_entry(pack_entry: input::Entry) -> data::Entry {
    data::Entry {
        header: pack_entry.header,
        decompressed_size: pack_entry.decompressed_size,
        data_offset: pack_entry.pack_offset + pack_entry.header_size as u64,
    }
}

fn resolve_delta(
    oid: &gix_hash::oid,
    out: &mut Vec<u8>,
    known_objects: &ObjectMap,
) -> Option<ResolvedBase> {
    known_objects.get(oid).map(|(bytes, kind)| {
        out.extend_from_slice(bytes);
        ResolvedBase::OutOfPack {
            kind: kind.clone(),
            end: out.len(),
        }
    })
}

async fn fetch_prereq_objects(
    pack_file: &data::File,
    ctx: &CoreContext,
    blobstore: Arc<RepoBlobstore>,
) -> Result<ObjectMap> {
    // Iterate over all packfile entries and fetch all the required base items
    let mut base_items = HashSet::new();
    let pack_stream = pack_file
        .streaming_iter()
        .context("Failure in iterating packfile")?;
    for entry in pack_stream {
        let entry = entry.context("Invalid packfile entry")?;
        if let RefDelta { base_id } = entry.header {
            base_items.insert(base_id);
        }
    }
    stream::iter(base_items)
        .map(Ok)
        .try_filter_map(|object_id| {
            cloned!(ctx, blobstore);
            async move {
                let git_identifier =
                    GitIdentifier::Basic(GitSha1::from_object_id(object_id.as_ref())?);
                let fallible_git_bytes =
                    fetch_git_object_bytes(&ctx, blobstore, &git_identifier, HeaderState::Included)
                        .await;
                match fallible_git_bytes {
                    Ok(git_bytes) => {
                        let kind = ObjectRef::from_loose(&git_bytes)
                            .context("Failure in converting bytes into git object")?
                            .kind();
                        anyhow::Ok(Some((object_id, (git_bytes, kind))))
                    }
                    // The object might not be present in the data store since its an inpack object
                    _ => anyhow::Ok(None),
                }
            }
        })
        .try_collect::<HashMap<_, _>>()
        .await
}

/// Method responsible for parsing the packfile provided as part of push, verifying its correctness
/// and returning a stream of objects contained within the packfile
pub async fn parse_pack(
    pack_bytes: &[u8],
    ctx: &CoreContext,
    blobstore: Arc<RepoBlobstore>,
) -> Result<impl Stream<Item = Result<Bytes>>> {
    let mut raw_file = Builder::new().suffix(".pack").rand_bytes(8).tempfile()?;
    raw_file.write_all(pack_bytes)?;
    raw_file.flush()?;
    let pack_file = File::at(raw_file.path(), Kind::Sha1).with_context(|| {
        format!(
            "Error while opening packfile for push at {}",
            raw_file.path().display()
        )
    })?;
    // Verify that the packfile is valid
    pack_file
        .verify_checksum(Discard, &AtomicBool::new(false))
        .context("The checksum of the packfile is invalid")?;

    // Load all the prerequisite objects
    let prereq_objects = fetch_prereq_objects(&pack_file, ctx, blobstore.clone()).await?;

    let stream = stream::iter(
        pack_file
            .streaming_iter()
            .context("Failure in iterating packfile")?,
    )
    .map(move |fallible_entry| match fallible_entry {
        Ok(entry) => {
            let mut output = vec![];
            let err_context = format!("Error in decoding packfile entry: {:?}", &entry.header);
            pack_file
                .decode_entry(
                    into_data_entry(entry),
                    &mut output,
                    |oid, out| resolve_delta(oid, out, &prereq_objects),
                    &mut Never,
                )
                .context(err_context)?;
            anyhow::Ok(Bytes::from(output))
        }
        Err(e) => anyhow::bail!("Failure in iterating packfile entry: {:?}", e),
    });
    raw_file.close().unwrap_or_default();
    Ok(stream)
}
