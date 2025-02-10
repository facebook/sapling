/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use data::entry::Header::RefDelta;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use git_types::fetch_git_object;
use git_types::GitIdentifier;
use git_types::ObjectContent;
use gix_features::progress::Discard;
use gix_hash::Kind;
use gix_hash::ObjectId;
use gix_object::encode::loose_header;
use gix_object::WriteTo;
use gix_pack::cache::Never;
use gix_pack::data;
use gix_pack::data::decode::entry::ResolvedBase;
use gix_pack::data::decode::Error as PackError;
use gix_pack::data::input::Entry as InputEntry;
use gix_pack::data::input::Error as InputError;
use gix_pack::data::File;
use mononoke_types::hash::GitSha1;
use packfile::types::BaseObject;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use repo_blobstore::RepoBlobstore;
use rustc_hash::FxHashMap;
use scuba_ext::FutureStatsScubaExt;
use tempfile::Builder;

use crate::PACKFILE_SUFFIX;

const MAX_ALLOWED_DEPTH: u8 = 30;
type ObjectMap = HashMap<ObjectId, ObjectContent>;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PackEntry {
    Pending(InputEntry),
    Processed((ObjectId, ObjectContent)),
}

#[derive(Debug, Clone, Default)]
struct PackEntries {
    entries: HashSet<PackEntry>,
}

impl PackEntries {
    fn from_pending_and_processed(pending: Vec<InputEntry>, processed: ObjectMap) -> Self {
        let mut entries = HashSet::new();
        for entry in pending {
            entries.insert(PackEntry::Pending(entry));
        }
        for (id, object_content) in processed {
            entries.insert(PackEntry::Processed((id, object_content)));
        }
        Self { entries }
    }

    fn from_entries(entries: HashSet<PackEntry>) -> Self {
        Self { entries }
    }

    fn into_pending_and_processed(self) -> (Vec<InputEntry>, ObjectMap) {
        let mut pending = Vec::new();
        let mut processed = HashMap::new();
        for entry in self.entries {
            match entry {
                PackEntry::Pending(entry) => pending.push(entry),
                PackEntry::Processed((id, content)) => {
                    processed.insert(id, content);
                }
            }
        }
        (pending, processed)
    }

    fn into_processed(self) -> Result<FxHashMap<ObjectId, ObjectContent>> {
        let mut object_map = FxHashMap::default();
        for entry in self.entries {
            match entry {
                PackEntry::Processed((id, content)) => {
                    object_map.insert(id, content);
                }
                _ => anyhow::bail!("Packfile entries are not completely processed"),
            }
        }
        Ok(object_map)
    }

    fn is_processed(&self) -> bool {
        self.entries.iter().all(|entry| match entry {
            PackEntry::Processed(_) => true,
            _ => false,
        })
    }
}

fn into_data_entry(pack_entry: InputEntry) -> data::Entry {
    data::Entry {
        header: pack_entry.header,
        decompressed_size: pack_entry.decompressed_size,
        data_offset: pack_entry.pack_offset + pack_entry.header_size as u64,
    }
}

/// Generates the full bytes of a git object including its header
fn git_object_bytes(
    headerless_object_bytes: Vec<u8>,
    kind: gix_object::Kind,
    size: usize,
) -> Vec<u8> {
    let mut object_bytes = loose_header(kind, size as u64).into_vec();
    object_bytes.extend(headerless_object_bytes);
    object_bytes
}

fn resolve_delta(
    oid: &gix_hash::oid,
    out: &mut Vec<u8>,
    known_objects: &ObjectMap,
) -> Option<ResolvedBase> {
    known_objects.get(oid).map(|object_content| {
        object_content.parsed.write_to(out.by_ref()).unwrap();
        ResolvedBase::OutOfPack {
            kind: object_content.parsed.kind().clone(),
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
                let fallible_git_object = fetch_git_object(&ctx, blobstore, &git_identifier).await;
                match fallible_git_object {
                    Ok(object) => {
                        let mut git_bytes = object.loose_header().into_vec();
                        object.write_to(git_bytes.by_ref())?;
                        anyhow::Ok(Some((
                            object_id,
                            ObjectContent::new(object, Bytes::from(git_bytes)),
                        )))
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
/// and returning an object map containing all the objects present in packfile mapped to their IDs
pub async fn parse_pack(
    pack_bytes: &[u8],
    ctx: &CoreContext,
    blobstore: Arc<RepoBlobstore>,
) -> Result<FxHashMap<ObjectId, ObjectContent>> {
    // If the packfile is empty, return an empty object map. This can happen when the push only has ref create/update
    // pointing to existing commit or just ref deletes
    if pack_bytes.is_empty() {
        return Ok(FxHashMap::default());
    }
    let mut raw_file = Builder::new()
        .suffix(PACKFILE_SUFFIX)
        .rand_bytes(8)
        .tempfile()?;
    raw_file.write_all(pack_bytes)?;
    raw_file.flush()?;
    let response = parse_stored_pack(raw_file.path(), ctx, blobstore).await;
    raw_file.close().unwrap_or_default();
    response
}

fn process_pack_entries(pack_file: &data::File, entries: PackEntries) -> Result<PackEntries> {
    let (pending_entries, prereq_objects) = entries.into_pending_and_processed();
    let output_entries = pending_entries
        .into_par_iter()
        .map(|entry| {
            let mut output = vec![];
            let err_context = format!("Error in decoding packfile entry: {:?}", &entry.header);
            let outcome = pack_file.decode_entry(
                into_data_entry(entry.clone()),
                &mut output,
                &mut gix_features::zlib::Inflate::default(),
                &|oid, out| resolve_delta(oid, out, &prereq_objects),
                &mut Never,
            );
            match outcome {
                Ok(outcome) => {
                    let object_bytes = Bytes::from(git_object_bytes(
                        output,
                        outcome.kind,
                        outcome.object_size as usize,
                    ));
                    let base_object = BaseObject::new(object_bytes.clone())
                        .context("Error in converting bytes to git object")?;
                    let id = base_object.hash;
                    let object = ObjectContent::new(base_object.object, object_bytes);
                    let processed_entry = PackEntry::Processed((id, object));
                    anyhow::Ok(processed_entry)
                }
                Err(e) => match e {
                    PackError::DeltaBaseUnresolved(_) => anyhow::Ok(PackEntry::Pending(entry)),
                    _ => Err(e).context(err_context),
                },
            }
        })
        .collect::<Result<HashSet<PackEntry>>>()
        .context("Failure in decoding packfile entries")?;
    let output_entries = prereq_objects
        .into_iter()
        .map(|(id, object_content)| PackEntry::Processed((id, object_content)))
        .chain(output_entries)
        .collect();
    Ok(PackEntries::from_entries(output_entries))
}

async fn parse_stored_pack(
    pack_path: &Path,
    ctx: &CoreContext,
    blobstore: Arc<RepoBlobstore>,
) -> Result<FxHashMap<ObjectId, ObjectContent>> {
    let pack_file = Arc::new(File::at(pack_path, Kind::Sha1).with_context(|| {
        format!(
            "Error while opening packfile for push at {}",
            pack_path.display()
        )
    })?);
    // Verify that the packfile is valid
    tokio::task::spawn_blocking({
        let pack_file = pack_file.clone();
        move || {
            pack_file
                .verify_checksum(&mut Discard, &AtomicBool::new(false))
                .context("The checksum of the packfile is invalid")
        }
    })
    .try_timed()
    .await?
    .log_future_stats(ctx.scuba().clone(), "Verified Packfile Checksum", None)?;

    // Load all the prerequisite objects
    let prereq_objects = fetch_prereq_objects(&pack_file, ctx, blobstore.clone())
        .try_timed()
        .await?
        .log_future_stats(ctx.scuba().clone(), "Fetched Prerequisite Objects", None);
    // Fetch all the entries that need to be processed
    let pending_entries = pack_file
        .streaming_iter()
        .context("Failure in iterating packfile")?
        .collect::<Result<Vec<_>, InputError>>()?;

    // Process all the entries
    tokio::task::spawn_blocking({
        let pack_file = pack_file.clone();
        move || {
            let mut pack_entries =
                PackEntries::from_pending_and_processed(pending_entries, prereq_objects);
            let mut counter = 0;
            while !pack_entries.is_processed() {
                if counter > MAX_ALLOWED_DEPTH {
                    anyhow::bail!(
                        "Maximum allowed depth reached while processing packfile entries"
                    );
                }
                counter += 1;
                pack_entries = process_pack_entries(&pack_file, pack_entries)?;
            }
            pack_entries.into_processed()
        }
    })
    .try_timed()
    .await?
    .log_future_stats(ctx.scuba().clone(), "Decoded objects from Packfile", None)
}
