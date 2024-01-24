/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use futures::Stream;
use futures::StreamExt;
use gix_hash::ObjectId;
use gix_pack::data::header;
use gix_pack::data::output::Entry;
use gix_pack::data::Version;
use thiserror::Error;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::hash_writer::AsyncHashWriter;
use crate::types::PackfileItem;

#[derive(Error, Debug)]
#[error(transparent)]
pub struct PackfileError(#[from] anyhow::Error);

/// The final representation of deltas in the packfile
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaForm {
    /// The deltas in the packfile can be either RefDelta or OffsetDelta
    RefAndOffset,
    /// All the deltas in packfile should be OffsetDeltas. Any RefDelta will be
    /// converted to OffsetDelta
    OnlyOffset,
}

/// Struct responsible for encoding and writing incoming stream
/// of git object bytes as a packfile to `raw_writer`.
pub struct PackfileWriter<T>
where
    T: AsyncWrite + Unpin,
{
    /// Writer used for writing the raw byte content of packfile
    pub hash_writer: AsyncHashWriter<T>,
    /// The number of git object entries in the packfile written so far
    pub num_entries: u32,
    /// The size of the packfile in bytes written so far
    pub size: u64,
    /// The concurrency with which the stream will be prefetched for writing
    /// to the underlying writer
    pub concurrency: usize,
    /// The hash of all the Object Ids in the packfile which will be generated
    /// when writing to the packfile has completed
    pub hash: Option<ObjectId>,
    /// The header information to be written at the beginning of the packfile.
    /// Once the header has been written, this field is set to None.
    header_info: Option<(Version, u32)>,
    /// Entries marking the offset at which each object in the packfile begins
    /// along with flag determining if the object actually exists at the offset.
    object_offset_with_validity: Vec<(u64, bool)>,
    /// The form of deltas that should be allowed in the packfile
    delta_form: DeltaForm,
    /// Mapping from Object Id to index in `object_offset_with_validity`
    object_id_with_index: HashMap<ObjectId, usize>,
}

#[allow(dead_code)]
impl<T: AsyncWrite + Unpin> PackfileWriter<T> {
    /// Create a new packfile writer based on `raw_writer` for writing `count` entries to the Packfile.
    pub fn new(raw_writer: T, count: u32, concurrency: usize, delta_form: DeltaForm) -> Self {
        let hash_writer = AsyncHashWriter::new(raw_writer);
        Self {
            hash_writer,
            num_entries: 0,
            size: 0,
            hash: None,
            concurrency,
            // Git uses V2 right now so we do the same
            header_info: Some((Version::V2, count)),
            object_offset_with_validity: Vec::with_capacity(count as usize),
            object_id_with_index: HashMap::with_capacity(count as usize),
            delta_form,
        }
    }

    /// Write the packfile header information if it hasn't been written yet.
    async fn write_header(&mut self) -> Result<()> {
        if let Some((version, count)) = self.header_info.take() {
            let header_bytes = header::encode(version, count);
            self.hash_writer.write_all(&header_bytes[..]).await?;
            self.size += header_bytes.len() as u64;
        }
        Ok(())
    }

    /// Write the stream of objects to the packfile
    pub async fn write(
        &mut self,
        entries_stream: impl Stream<Item = Result<PackfileItem>>,
    ) -> Result<()> {
        use futures::TryStreamExt;
        // Write the packfile header if applicable
        self.write_header().await?;
        let mut entries_stream = Box::pin(entries_stream.ready_chunks(self.concurrency));
        while let Some(entries) = entries_stream.next().await {
            let entries: Vec<_> = futures::stream::iter(entries)
                .map_ok(|entry| async move {
                    let entry: Entry = tokio::task::spawn_blocking(move || {
                        entry
                            .try_into()
                            .context("Failure in converting PackfileItem to Entry")
                    })
                    .await??;
                    anyhow::Ok(entry)
                })
                .try_buffered(self.concurrency)
                .try_collect()
                .await?;

            for mut entry in entries {
                // If the entry is already written to the packfile, skip writing it again
                if self.object_id_with_index.contains_key(&entry.id) {
                    continue;
                }
                // Will be false for all our cases since we generate the entry with the object ID in hand. Including here for
                // completeness sake.
                if entry.is_invalid() {
                    self.object_offset_with_validity.push((0, false));
                }
                // The current object will be written at offset `size`.
                self.object_offset_with_validity.push((self.size, true));
                self.object_id_with_index
                    .insert(entry.id.clone(), self.object_offset_with_validity.len() - 1);
                if let DeltaForm::OnlyOffset = self.delta_form {
                    // The pack is allowed to have only offset deltas. Convert any ref deltas into
                    // offset deltas before writing to pack
                    entry = self.convert_ref_delta_to_offset_delta(entry)?;
                }
                // Since the packfile is version 2, the entry should follow the same version
                let header = entry.to_entry_header(Version::V2, |index| {
                    let (base_offset, is_valid_object) = self.object_offset_with_validity[index];
                    if !is_valid_object {
                        unreachable!("Encountered a RefDelta that points to an object which does not exist in the packfile.")
                    }
                    self.size - base_offset
                });
                // Write the header to a vec buffer instead of writing directly to hash_writer since the Header type expects
                // an impl Write instance and not an impl AsyncWrite instance. This is fine since the header is always a handful of bytes.
                let mut header_buffer = Vec::new();
                let header_written_size =
                    header.write_to(entry.decompressed_size as u64, &mut header_buffer.by_ref())?;
                // Write the header to the async hash writer
                self.hash_writer
                    .write_all(&header_buffer[..header_written_size])
                    .await?;
                // Record the written bytes
                self.size += header_written_size as u64;
                // Write the compressed contents of the entry to the packfile
                self.size +=
                    tokio::io::copy(&mut &*entry.compressed_data, &mut self.hash_writer).await?;
                // Increment the number of entries written in the packfile
                self.num_entries += 1;
            }
        }
        Ok(())
    }

    /// Finish the packfile by writing the trailer at the end and returning the checksum
    /// hash of the generated file.
    pub async fn finish(&mut self) -> Result<ObjectId> {
        // Get the hash of all the content written so far
        let digest = self.hash_writer.hasher.clone().digest();
        // Append the hash to the end of the packfile as a checksum
        self.hash_writer.inner.write_all(&digest[..]).await?;
        self.size += digest.len() as u64;
        self.hash_writer.inner.flush().await?;
        // Update the hash for the writer indicating that we have finished writing
        self.hash = Some(ObjectId::from(digest));
        Ok(ObjectId::from(digest))
    }

    /// Consumes the instance after writing the packfile and returns
    /// the underlying raw writer.
    pub fn into_write(self) -> T {
        self.hash_writer.inner
    }

    fn convert_ref_delta_to_offset_delta(&self, entry: Entry) -> Result<Entry> {
        match entry.kind {
            gix_pack::data::output::entry::Kind::Base(_) => Ok(entry),
            gix_pack::data::output::entry::Kind::DeltaRef { .. } => Ok(entry),
            gix_pack::data::output::entry::Kind::DeltaOid { id } => {
                let object_index = self
                    .object_id_with_index
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("Couldn't find index for {}", id))?
                    .clone();
                let kind = gix_pack::data::output::entry::Kind::DeltaRef { object_index };
                Ok(Entry { kind, ..entry })
            }
        }
    }
}
