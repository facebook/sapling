/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Uses https://git-scm.com/docs/pack-format#_deltified_representation as source
//! NOTE: We can represent Git objects as Deltas only if the size of the objects is less than 4GB

use std::cmp::Ordering;
use std::ops::Range;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use blobstore::BlobstoreBytes;
use bytes::Bytes;
use fbthrift::compact_protocol;
use gix_diff::blob::diff;
use gix_diff::blob::intern::InternedInput;
use gix_diff::blob::intern::TokenSource;
use gix_diff::blob::sink::Sink;
use gix_diff::blob::Algorithm;
use mononoke_types::path::MPath;
use mononoke_types::private::Blake2;
use mononoke_types::private::MononokeTypeError;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChangesetId;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::thrift;

/// The maximum size of raw bytes that can be contained within a single
/// Data instruction
const MAX_DATA_BYTES: usize = (1 << 7) - 1;
/// The maximum number of bytes that can be copied from a base object to a new object
/// as part of a single Copy instruction
const MAX_COPY_BYTES: u32 = (1 << 24) - 1;
/// Bit-level flag indicating that more bytes will follow the current byte for representing
/// some data
const CONTINUATION_BITMASK: u8 = 1 << 7;
/// Bit-level flag identifying a Copy instruction. The flag for Data instruction is 0
const COPY_INSTRUCTION: u8 = 1 << 7;
/// Bitmask representing the section of the byte which contains just data and no flags
const DATA_BITMASK: u8 = (1 << 7) - 1;
/// Specific range size within a copy instruction which is encoded uniquely by Git, ignoring
/// the standard format
const COPY_SPECIAL_SIZE: u32 = 1 << 16;
/// The hashing key used for generating the final hash for a delta instruction chunk
const DELTA_INSTRUCTION_HASHING_KEY: &str = "git_delta_instruction_chunk";
/// The maximum number of chunks for a file that will be considered for computing deltas
/// This ensures that even big files can be deltaed quickly
const DELTA_CHUNK_COUNT: usize = 100_000;

/// Identifier for accessing a specific delta instruction chunk from the blobstore
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct DeltaInstructionChunkId(Blake2);

#[allow(dead_code)]
impl DeltaInstructionChunkId {
    pub fn new(
        actual_cs_id: &ChangesetId,
        actual_mpath: &MPath,
        base_cs_id: &ChangesetId,
        base_mpath: &MPath,
        index: usize,
    ) -> Self {
        let mut blake2 =
            mononoke_types::hash::Context::new(DELTA_INSTRUCTION_HASHING_KEY.as_bytes());
        blake2.update(actual_cs_id);
        blake2.update(actual_mpath.to_null_separated_bytes());
        blake2.update(base_cs_id);
        blake2.update(base_mpath.to_null_separated_bytes());
        blake2.update(index.to_be_bytes());
        Self(blake2.finish())
    }

    pub fn from_hash(hash: Blake2) -> Self {
        Self(hash)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeltaInstructionChunkIdPrefix {
    actual_cs_id: ChangesetId,
    actual_mpath: MPath,
    base_cs_id: ChangesetId,
    base_mpath: MPath,
}

#[allow(dead_code)]
impl DeltaInstructionChunkIdPrefix {
    pub fn new(
        actual_cs_id: ChangesetId,
        actual_mpath: MPath,
        base_cs_id: ChangesetId,
        base_mpath: MPath,
    ) -> Self {
        Self {
            actual_cs_id,
            actual_mpath,
            base_cs_id,
            base_mpath,
        }
    }

    pub fn as_id(&self, index: usize) -> DeltaInstructionChunkId {
        DeltaInstructionChunkId::new(
            &self.actual_cs_id,
            &self.actual_mpath,
            &self.base_cs_id,
            &self.base_mpath,
            index,
        )
    }
}

impl FromStr for DeltaInstructionChunkId {
    type Err = anyhow::Error;
    #[inline]
    fn from_str(s: &str) -> anyhow::Result<Self> {
        Blake2::from_str(s).map(Self::from_hash)
    }
}

impl BlobstoreKey for DeltaInstructionChunkId {
    fn blobstore_key(&self) -> String {
        format!("{}.blake2.{}", DELTA_INSTRUCTION_HASHING_KEY, self.0)
    }

    fn parse_blobstore_key(key: &str) -> Result<Self> {
        let prefix = format!("{}.blake2", DELTA_INSTRUCTION_HASHING_KEY);
        match key.strip_prefix(&prefix) {
            None => anyhow::bail!("{} is not a blobstore key for DeltaInstructionChunkId", key),
            Some(suffix) => Self::from_str(suffix),
        }
    }
}

/// Type representing a chunk of delta instructions stored as an individual unit in the blobstore
#[derive(Clone, Eq, PartialEq)]
pub struct DeltaInstructionChunk(Bytes);

#[allow(dead_code)]
impl DeltaInstructionChunk {
    pub fn new_bytes<B: Into<Bytes>>(b: B) -> Self {
        DeltaInstructionChunk(b.into())
    }

    pub(crate) fn from_thrift(chunk: thrift::DeltaInstructionChunk) -> Result<Self> {
        Ok(DeltaInstructionChunk(chunk.0))
    }

    pub(crate) fn into_thrift(self) -> thrift::DeltaInstructionChunk {
        thrift::DeltaInstructionChunk(self.0)
    }

    pub fn from_encoded_bytes(encoded_bytes: Bytes) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(encoded_bytes).with_context(|| {
            MononokeTypeError::BlobDeserializeError("DeltaInstructionChunk".into())
        })?;
        Self::from_thrift(thrift_tc)
    }

    pub fn into_blobstore_bytes(self) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&self.into_thrift()))
    }

    pub fn size(&self) -> u64 {
        // NOTE: This panics if the buffer length doesn't fit a u64: that's fine.
        self.0.len().try_into().unwrap()
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

/// Individual instruction for constructing a part of a
/// new object based on a base object
#[derive(Clone, Hash, Eq, PartialEq)]
pub enum DeltaInstruction {
    /// Use raw data bytes from the new object
    Data(Bytes),
    /// Copy `usize` bytes starting at `base_offset` in the base object
    /// into the new object
    Copy { base_offset: u32, size: u32 },
}

#[allow(dead_code)]
impl DeltaInstruction {
    pub fn from_data(data: Bytes) -> Result<Self> {
        // Each data instruction can be used to write at max 127 bytes since
        // the size of the written bytes need to be represented by only 7 bits
        if data.len() > MAX_DATA_BYTES {
            anyhow::bail!("Encountered invalid data instruction size: {}", data.len())
        }
        Ok(Self::Data(data))
    }

    pub fn from_copy(byte_range: Range<u32>) -> Result<Self> {
        // As per the format requirements, the size of the range cannot be
        // empty
        if byte_range.is_empty() {
            anyhow::bail!(
                "Encountered empty range {:?} for copy instruction",
                byte_range
            );
        }
        let size = byte_range.len() as u32;
        // Additionally, the size of the range cannot exceed 1^24 - 1 bytes
        // since at max only 3 bytes can be used to represent the size
        if size > MAX_COPY_BYTES {
            anyhow::bail!("Encountered invalid size {} for copy instruction", size);
        }
        // The offset is required to be constrained under 4 bytes but since its represented
        // via a u32, the type system enforces that check for us
        Ok(Self::Copy {
            base_offset: byte_range.start,
            size,
        })
    }

    pub async fn write(&self, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        // A single Data or Copy instruction can have maximum size of 128 bytes. Instead of writing individual
        // bytes to the out writer (which can be expensive depending upon the type of writer), we write them
        // to a Vec buffer which would then be one-time flushed to the out writer at the end.
        let mut buffer = Vec::with_capacity(MAX_DATA_BYTES + 1);
        match self {
            DeltaInstruction::Data(ref bytes) => {
                // Data instructions start with the 8th bit of the first byte set to 0
                // The remaining 7 bits represent the size of the raw data associated with this instruction
                // Maximum 127 bytes of data can follow as part of this instruction
                let encoded_instruction: u8 = bytes.len() as u8;
                buffer.write_all(&[encoded_instruction]).await?;
                buffer.write_all(bytes).await?;
            }
            DeltaInstruction::Copy { base_offset, size } => {
                // Copy instructions can be encoded using max 8 bytes out of which
                // the first byte will be used to identify the type of instruction and
                // the number of offset and size bytes that will follow. Offset can be
                // represented by max 4 bytes and size can be represented by max 3 bytes.
                let mut instruction_byte = COPY_INSTRUCTION;

                // Write the offset bytes in little endian order
                let offset_bytes = base_offset.to_le_bytes();
                // Git creates an exception to this format where if size = 65536,
                // instead of encoding it as [0,0,1] in LE bytes we encode it as
                // [0,0,0]. Since no valid object size can be 0, Git skips allocating
                // even a byte for the size field for the special case of 65536
                let size = if *size == COPY_SPECIAL_SIZE {
                    0u32
                } else {
                    *size
                };
                // Write the size bytes in little endian order
                let size_bytes = size.to_le_bytes();
                // For each byte position of offset_bytes and size_bytes that has a non-zero value,
                // set the corresponding bit in instruction_byte
                for (idx, &byte) in offset_bytes.iter().chain(size_bytes.iter()).enumerate() {
                    if byte != 0 {
                        instruction_byte |= 1 << idx;
                    }
                }
                // Write the instruction_byte to out
                buffer.write_all(&[instruction_byte]).await?;
                // Write the non-zero offset bytes to out
                for byte in offset_bytes {
                    if byte != 0 {
                        buffer.write_all(&[byte]).await?;
                    }
                }
                // Write the non-zero size bytes to out
                for byte in size_bytes {
                    if byte != 0 {
                        buffer.write_all(&[byte]).await?;
                    }
                }
            }
        }
        // Finally, flush the buffer to out
        out.write_all(&buffer).await?;
        Ok(())
    }
}

impl std::fmt::Debug for DeltaInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data(bytes) => f.write_fmt(format_args!(
                "Data instruction: {}",
                String::from_utf8_lossy(bytes.as_ref())
            )),
            Self::Copy { base_offset, size } => f.write_fmt(format_args!(
                "Copy instruction: base_offset: {:?}, size: {:?}",
                base_offset, size
            )),
        }
    }
}

/// List of instructions which when applied in order form a
/// complete new object based on delta of a base object
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct DeltaInstructions {
    base_object: Bytes,
    base_object_chunk_size: usize,
    new_object: Bytes,
    new_object_chunk_size: usize,
    processed_till: u32, // To keep track of the byte position till which the delta has been processed
    instructions: Vec<DeltaInstruction>,
}

#[allow(dead_code)]
impl DeltaInstructions {
    // Generate set of DeltaInstructions for the given base and new object by diffing them
    // using the provided diff algorithm
    pub fn generate(
        base_object: Bytes,
        new_object: Bytes,
        diff_algorithm: Algorithm,
    ) -> Result<Self> {
        let base_object_vec = base_object.to_vec();
        let new_object_vec = new_object.to_vec();
        let tokened_base_object = ObjectData::new(&base_object_vec);
        let tokened_new_object = ObjectData::new(&new_object_vec);
        let delta_instructions = Self {
            base_object,
            new_object,
            base_object_chunk_size: tokened_base_object.chunk_size(),
            new_object_chunk_size: tokened_new_object.chunk_size(),
            instructions: Vec::new(),
            processed_till: 0,
        };
        let interned_input = InternedInput::new(tokened_base_object, tokened_new_object);
        let fallible_delta_instructions = FallibleDeltaInstructions::Valid(delta_instructions);
        diff(diff_algorithm, &interned_input, fallible_delta_instructions)
    }

    pub async fn write(&self, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        // Write the size of the base object
        write_size(self.base_object.len(), out).await?;
        // Write the size of the new object
        write_size(self.new_object.len(), out).await?;
        // Write the delta instructions in order
        self.write_instructions(out).await
    }

    pub async fn write_instructions(&self, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        for instruction in self.instructions.iter() {
            instruction.write(out).await?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for DeltaInstructions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "DeltaInstructions:\n base_object: {},\n new_object: {},\n processed_till: {}\n",
            String::from_utf8_lossy(self.base_object.as_ref()),
            String::from_utf8_lossy(self.new_object.as_ref()),
            self.processed_till,
        ))?;
        for instruction in self.instructions.iter() {
            f.write_fmt(format_args!("{:?}\n", instruction))?;
        }
        Ok(())
    }
}

/// Enum representing Delta Instructions that can be either be valid or invalid.
/// If valid, they contain the actual instructions. If invalid, they contain the
/// underlying error
#[allow(dead_code)]
pub enum FallibleDeltaInstructions {
    Valid(DeltaInstructions),
    Invalid(anyhow::Error),
}

impl FallibleDeltaInstructions {
    /// Convert to Result and return an error if the instruction is invalid
    fn into_result(self) -> Result<DeltaInstructions> {
        match self {
            FallibleDeltaInstructions::Valid(v) => Ok(v),
            FallibleDeltaInstructions::Invalid(e) => Err(e),
        }
    }

    /// Add Copy instruction to the list of instructions if it is valid
    fn add_copy(&mut self, range: Range<u32>) {
        match self {
            Self::Valid(delta_instructions) => {
                match DeltaInstruction::from_copy(range.clone()) {
                    Ok(copy_instruction) => {
                        delta_instructions.instructions.push(copy_instruction);
                        delta_instructions.processed_till = range.end;
                    }
                    // If the data is invalid, then we should stop processing
                    Err(e) => {
                        *self = Self::Invalid(e);
                    }
                }
            }
            // If the instructions are already invalid, do nothing
            Self::Invalid(_) => {}
        }
    }

    /// Add Data instruction to the list of instructions if it is valid
    /// using the range of bytes from the new object
    pub fn add_data(&mut self, range: Range<u32>) {
        match self {
            Self::Valid(delta_instructions) => {
                let bytes = delta_instructions
                    .new_object
                    .slice((range.start as usize)..(range.end as usize));
                match DeltaInstruction::from_data(bytes) {
                    Ok(data_instruction) => {
                        delta_instructions.instructions.push(data_instruction);
                    }
                    // If the data is invalid, then we should stop processing
                    Err(e) => *self = Self::Invalid(e),
                };
            }
            // If the instructions are already invalid, do nothing
            Self::Invalid(_) => {}
        }
    }

    /// Update the processed_till field of the instructions if it is valid
    pub fn update_processed_till(&mut self, new_processed_till: u32) {
        match self {
            Self::Valid(delta_instructions) => {
                delta_instructions.processed_till = new_processed_till;
            }
            // If the instructions are already invalid, do nothing
            Self::Invalid(_) => {}
        }
    }
}

// Implement Sink for FallibleDeltaInstructions instead of DeltaInstructions since we can encounter
// errors during the processing of deltas and the trait signature involves infallible types
impl Sink for FallibleDeltaInstructions {
    type Out = Result<DeltaInstructions>;

    fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
        match self {
            Self::Valid(delta_instructions) => {
                // The before and after ranges are essentially chunk indices where each
                // chunk can be `chunk_size` bytes long. To get the actual byte level index,
                // we need to multiply the `chunk_size` with the chunk index
                let base_object_offset = delta_instructions.base_object_chunk_size as u32;
                let base_object_len = delta_instructions.base_object.len() as u32;
                let before = (before.start * base_object_offset)
                    ..(std::cmp::min(before.end * base_object_offset, base_object_len));

                let new_object_offset = delta_instructions.new_object_chunk_size as u32;
                let new_object_len = delta_instructions.new_object.len() as u32;
                let after = (after.start * new_object_offset)
                    ..(std::cmp::min(after.end * new_object_offset, new_object_len));
                let processed_till = delta_instructions.processed_till.clone();
                // Every change detected by the algorithm would be represented as a Data instruction since
                // the changed part of the content cannot be copied from the base object. The data instruction
                // can be preceded by a Copy instruction if the range prior to `before` was not covered already.
                match before.start.cmp(&processed_till) {
                    // The start of this delta range has already been processed before. Since the ranges are
                    // monotonically increasing, this should not happen and is likely the result of a bug.
                    Ordering::Less => {
                        *self = Self::Invalid(anyhow::anyhow!(
                            "Encountered invalid processed range {:?} while diffing content",
                            before
                        ));
                        return;
                    }
                    // The delta range starts exactly where we ended our previous processing. In this case,
                    // we do nothing since no copy instructions need to be prepended before the data instruction.
                    Ordering::Equal => {}
                    // There exists a gap between our previously processed endpoint and the start of this delta range.
                    // This indicates the section of content lying between this range needs to be copied from the base
                    // object
                    Ordering::Greater => {
                        // Since the range from processed_till..range_start can be too large to be covered
                        // by a single copy instruction, we need to split the range into mini-ranges of size
                        // MAX_COPY_BYTES or less. Each such mini-range will be a Copy instruction that will
                        // be added to the list of instructions
                        let range_start = before.start;
                        let mut copied_till = processed_till;
                        for subrange_start in
                            (processed_till..range_start).step_by(MAX_COPY_BYTES as usize)
                        {
                            copied_till = std::cmp::min(
                                range_start,
                                subrange_start.saturating_add(MAX_COPY_BYTES),
                            );
                            self.add_copy(subrange_start..copied_till);
                        }
                        // Add copy instruction for the remaining subrange
                        if copied_till < range_start {
                            self.add_copy(copied_till..range_start);
                        }
                    }
                }
                // Now that the Copy instructions are added, append the data instruction for this range
                // of changed content. Note that if the amount of raw bytes to be added exceeds the limit
                // of MAX_DATA_BYTES, we would need to split the range into mini-ranges of size
                // MAX_DATA_BYTES or less. Each such mini-range will be a Data instruction that will
                // be added to the list of instructions
                let range_start = after.start;
                let mut written_till = range_start;
                for subrange_start in after.clone().step_by(MAX_DATA_BYTES) {
                    written_till = std::cmp::min(
                        after.end,
                        subrange_start.saturating_add(MAX_DATA_BYTES as u32),
                    );
                    self.add_data(subrange_start..written_till);
                }
                // Add data instruction for the remaining subrange
                if written_till < after.end {
                    self.add_data(written_till..after.end);
                }
                // Record that we have processed the entire range
                self.update_processed_till(before.end);
            }
            // If we have already encountered an error, don't process any further deltas
            Self::Invalid(_) => {}
        }
    }

    fn finish(mut self) -> Self::Out {
        if let Self::Valid(delta_instructions) = &mut self {
            let base_obj_len = delta_instructions.base_object.len() as u32;
            let processed_till = delta_instructions.processed_till;
            match base_obj_len.cmp(&processed_till) {
                Ordering::Less => {
                    // We have processed more than the size of the base object. This should
                    // not happen and is likely the result of a bug
                    anyhow::bail!(
                        "Processed till position {} which is greater than base object size {}",
                        processed_till,
                        base_obj_len
                    )
                }
                Ordering::Equal => {
                    // We have processed till the end of the base object as expected. Return the
                    // final set of delta instructions
                }
                Ordering::Greater => {
                    // We have not yet processed the entire base object. This can happen if the last
                    // section of the base object is the same for the new object, hence need to Copy
                    // the remaining contents
                    let mut copied_till = processed_till;
                    for subrange_start in
                        (processed_till..base_obj_len).step_by(MAX_COPY_BYTES as usize)
                    {
                        copied_till = std::cmp::min(
                            base_obj_len,
                            subrange_start.saturating_add(MAX_COPY_BYTES),
                        );
                        self.add_copy(subrange_start..copied_till);
                    }
                    // Add copy instruction for the remaining subrange
                    if copied_till < base_obj_len {
                        self.add_copy(copied_till..base_obj_len);
                    }
                }
            }
        }
        self.into_result()
    }
}

/// Wrapper type over the bytes representing the data of the Git Object, used
/// for bypassing the orphan rule for implementing the TokenSource trait
struct ObjectData<'a> {
    data: &'a Vec<u8>,
    chunk_size: usize,
}

impl<'a> ObjectData<'a> {
    pub fn new(data: &'a Vec<u8>) -> Self {
        let chunk_size = std::cmp::max(data.len() / DELTA_CHUNK_COUNT, 1);
        Self { data, chunk_size }
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

impl<'a> TokenSource for ObjectData<'a> {
    // Depending upon the input, the granularity could be individual bytes (for file less than 100KB)
    // or chunks of bytes (for large files)
    type Token = &'a [u8];

    type Tokenizer = std::slice::Chunks<'a, u8>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.data.chunks(self.chunk_size)
    }

    fn estimate_tokens(&self) -> u32 {
        (self.data.len() / self.chunk_size) as u32
    }
}

/// Write the size "size" using the size encoding scheme used by Git
/// The encoding scheme is one of variable length where the bytes are written
/// in little-endian order. Only the lower 7 bits of each byte are used to represent
/// the size data and the 8th bit is used to represent continuation.
async fn write_size(size_to_write: usize, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
    let mut size = size_to_write;
    // Get the first byte of size in little endian order ignoring the
    // 8th bit
    let mut byte: u8 = size as u8 & DATA_BITMASK;
    // Right shift size by 7 positions since we have already consumed 7 bits
    size >>= 7;
    // While size still remains to be encoded completely
    while size != 0 {
        // Since size is not yet zero we will definitely have follow up bytes
        // Hence in addition to the 7 data bits from size we write the 8th
        // continuation bit to indicate that we have follow up bytes
        out.write_all(&[byte | CONTINUATION_BITMASK]).await?;
        // Capture the next 7 bits
        byte = size as u8 & DATA_BITMASK;
        // Right shift size by 7 positions since we have already consumed 7 bits
        size >>= 7;
    }
    // Size is zero and the last captured byte has not yet been written. Write the
    // final byte to out but without the 8th bit set since there are no more bytes to
    // follow in the encoding
    out.write_all(&[byte]).await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use std::io::Write;
    use std::sync::Arc;

    use anyhow::Context;
    use anyhow::Result;
    use blobstore::Blobstore;
    use bytes::BytesMut;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use flate2::write::ZlibDecoder;
    use futures::TryStreamExt;
    use gix_hash::ObjectId;
    use gix_object::ObjectRef;
    use gix_object::Tag;
    use gix_object::WriteTo;
    use memblob::Memblob;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use rand::Rng;

    use super::*;
    use crate::fetch_delta_instructions;
    use crate::store::store_delta_instructions;
    /// Decode the encoded (base or new) object size from Git Delta Instructions header
    /// Originally from gix-pack pub function which is currently not exposed (https://fburl.com/5ucyqwvj)
    /// NOTE: For testing purposes only. Do not use in production.
    pub fn decode_header_size(d: &[u8]) -> (u64, usize) {
        let mut i = 0;
        let mut size = 0u64;
        let mut consumed = 0;
        for cmd in d.iter() {
            consumed += 1;
            size |= (*cmd as u64 & 0x7f) << i;
            i += 7;
            if *cmd & 0x80 == 0 {
                break;
            }
        }
        (size, consumed)
    }

    /// Apply delta instructions `data` to generate complete `target` object from `base` object
    /// Originally from gix-pack pub function which is currently not exposed (https://fburl.com/8ttcw83w)
    /// NOTE: For testing purposes only. Do not use in production.
    pub fn apply(base: &[u8], target: &mut Vec<u8>, data: &[u8]) {
        let mut i = 0;
        while let Some(cmd) = data.get(i) {
            i += 1;
            match cmd {
                cmd if cmd & 0b1000_0000 != 0 => {
                    let (mut ofs, mut size): (u32, u32) = (0, 0);
                    if cmd & 0b0000_0001 != 0 {
                        ofs = data[i] as u32;
                        i += 1;
                    }
                    if cmd & 0b0000_0010 != 0 {
                        ofs |= (data[i] as u32) << 8;
                        i += 1;
                    }
                    if cmd & 0b0000_0100 != 0 {
                        ofs |= (data[i] as u32) << 16;
                        i += 1;
                    }
                    if cmd & 0b0000_1000 != 0 {
                        ofs |= (data[i] as u32) << 24;
                        i += 1;
                    }
                    if cmd & 0b0001_0000 != 0 {
                        size = data[i] as u32;
                        i += 1;
                    }
                    if cmd & 0b0010_0000 != 0 {
                        size |= (data[i] as u32) << 8;
                        i += 1;
                    }
                    if cmd & 0b0100_0000 != 0 {
                        size |= (data[i] as u32) << 16;
                        i += 1;
                    }
                    if size == 0 {
                        size = 0x10000; // 65536
                    }
                    let ofs = ofs as usize;
                    std::io::Write::write_all(target, &base[ofs..ofs + size as usize])
                        .expect("delta copy from base: byte slices must match");
                }
                0 => panic!("encountered unsupported command code: 0"),
                size => {
                    std::io::Write::write_all(target, &data[i..i + *size as usize])
                        .expect("delta copy data: slice sizes to match up");
                    i += *size as usize;
                }
            }
        }
        assert_eq!(i, data.len());
    }

    async fn random_bytes_blob_delta_application(
        base_object_size: usize,
        new_object_size: usize,
    ) -> Result<()> {
        // Create an arbitrary set of bytes and use that as the base object
        let base_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(base_object_size)
            .collect();
        let base_object = Bytes::from(base_object);
        // Create an arbitrary set of bytes and use that as the new object
        let new_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(new_object_size)
            .collect();
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that the recreated_new_object matches the original new_object
        assert_eq!(new_object, Bytes::from(recreated_new_object));
        Ok(())
    }

    #[test]
    fn test_data_instruction_creation() -> Result<()> {
        // Creating a data instruction with more than 127 bytes of data should fail
        let data = [0u8; 128];
        let data_instruction = DeltaInstruction::from_data(Bytes::copy_from_slice(&data));
        assert!(data_instruction.is_err());
        // Validate creation of data instruction with valid data
        let data = [0u8; 127];
        let data_instruction = DeltaInstruction::from_data(Bytes::copy_from_slice(&data));
        assert!(data_instruction.is_ok());
        Ok(())
    }

    #[test]
    fn test_copy_instruction_creation() -> Result<()> {
        // Creating a copy instruction with an empty range should fail
        let empty_range = 32..32;
        let copy_instruction = DeltaInstruction::from_copy(empty_range);
        assert!(copy_instruction.is_err());
        // Creating a copy instruction with too wide a range should fail
        let too_large_range = 0..(MAX_COPY_BYTES + 1);
        let copy_instruction = DeltaInstruction::from_copy(too_large_range);
        assert!(copy_instruction.is_err());
        // Validate creation of copy instruction with valid range
        let valid_range = 0..MAX_COPY_BYTES;
        let copy_instruction = DeltaInstruction::from_copy(valid_range);
        assert!(copy_instruction.is_ok());
        Ok(())
    }

    #[test]
    fn test_basic_delta_creation() -> Result<()> {
        let base_object = Bytes::from_static(b"So close no matter how far");
        let new_object = Bytes::from_static(b"So close no matter if very far");
        let delta_instructions =
            DeltaInstructions::generate(base_object, new_object, Algorithm::Myers);
        // Validate that the delta instructions get created successfully
        assert!(
            delta_instructions.is_ok(),
            "Failure in creating delta instructions",
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_basic_delta_encoding() -> Result<()> {
        let base_object = Bytes::from_static(b"So close no matter how far");
        let new_object = Bytes::from_static(b"So close no matter if very far");
        let delta_instructions =
            DeltaInstructions::generate(base_object, new_object, Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        let result = delta_instructions.write(&mut encoded_instructions).await;
        assert!(result.is_ok(), "Failure in encoding delta instructions");
        Ok(())
    }

    #[fbinit::test]
    async fn test_basic_delta_application() -> Result<()> {
        let base_object = Bytes::from_static(b"So close no matter how far");
        let new_object = Bytes::from_static(b"So close no matter if very far");
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that the recreated_new_object matches the original new_object
        assert_eq!(new_object, Bytes::from(recreated_new_object));
        Ok(())
    }

    #[fbinit::test]
    async fn test_very_large_string_delta_application() -> Result<()> {
        // Create a 50 MB string with random characters
        let base_object: String = rand::thread_rng()
            .sample_iter::<char, _>(rand::distributions::Standard)
            .take(52_428_800)
            .collect();
        let base_bytes = Bytes::from(base_object.into_bytes());

        // Create another 40 MB string with random characters
        let new_object: String = rand::thread_rng()
            .sample_iter::<char, _>(rand::distributions::Standard)
            .take(41_943_040)
            .collect();
        let new_bytes = Bytes::from(new_object.into_bytes());

        let delta_instructions =
            DeltaInstructions::generate(base_bytes.clone(), new_bytes.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_bytes.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that the recreated_new_object matches the original new_object
        assert_eq!(new_bytes, Bytes::from(recreated_new_object));
        Ok(())
    }

    #[fbinit::test]
    async fn test_very_large_random_bytes_blob_delta_application() -> Result<()> {
        random_bytes_blob_delta_application(41_943_040, 52_428_800).await
    }

    #[fbinit::test]
    async fn test_mismatched_random_bytes_blob_delta_application() -> Result<()> {
        // Very small base object and large new object
        random_bytes_blob_delta_application(199, 2_097_152).await?;
        // Large base object and very small new object
        random_bytes_blob_delta_application(3_145_728, 271).await?;
        // Both small objects
        random_bytes_blob_delta_application(4_194_304, 4_194_305).await
    }

    #[fbinit::test]
    async fn test_smaller_base_random_bytes_blob_delta_application() -> Result<()> {
        // Create an arbitrary set of bytes and use that as the base object
        let base_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(100)
            .collect();
        let base_object = Bytes::from(base_object);
        // Create an arbitrary set of bytes and use that as the new object
        let new_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(10000)
            .collect();
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that the recreated_new_object matches the original new_object
        assert_eq!(new_object, Bytes::from(recreated_new_object));
        Ok(())
    }

    #[fbinit::test]
    async fn test_larger_base_random_bytes_blob_delta_application() -> Result<()> {
        // Create an arbitrary set of bytes and use that as the base object
        let base_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(10000)
            .collect();
        let base_object = Bytes::from(base_object);
        // Create an arbitrary set of bytes and use that as the new object
        let new_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(100)
            .collect();
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that the recreated_new_object matches the original new_object
        assert_eq!(new_object, Bytes::from(recreated_new_object));
        Ok(())
    }

    #[fbinit::test]
    async fn test_git_object_delta_application() -> Result<()> {
        // Create a Git tag object pointing to a tree and use it as base object
        let tag = Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Tree,
            name: "TreeTag".into(),
            tagger: None,
            message: "Tag pointing to a tree".into(),
            pgp_signature: None,
        };
        let mut base_object = tag.loose_header().into_vec();
        tag.write_to(base_object.by_ref())?;
        let base_object = Bytes::from(base_object);
        // Create a Git tag object pointing to a blob and use it as the new object
        let tag = Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Blob,
            name: "BlobTag".into(),
            tagger: None,
            message: "Tag pointing to a blob".into(),
            pgp_signature: None,
        };
        let mut new_object = tag.loose_header().into_vec();
        tag.write_to(new_object.by_ref())?;
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let mut encoded_instructions = Vec::new();
        delta_instructions
            .write_instructions(&mut encoded_instructions)
            .await?;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            encoded_instructions.as_ref(),
        );
        // Validate that we are able to recreate the Git tag object from
        // the delta-generated bytes
        let object = ObjectRef::from_loose(recreated_new_object.as_ref())
            .with_context(|| {
                format!(
                    "Error in deserialing bytes into Git Object: {}",
                    String::from_utf8_lossy(recreated_new_object.as_ref())
                )
            })?
            .to_owned();
        let output_tag = object
            .try_into_tag()
            .expect("Expected successful conversion into Git Tag");
        // Validate that the Git tag object obtained from the delta-generated bytes is the same
        // as the Tag object used as new_object above
        assert_eq!(tag, output_tag, "Git tag objects do not match");
        Ok(())
    }

    #[fbinit::test]
    async fn test_basic_delta_header() -> Result<()> {
        let base_object = Bytes::from_static(b"So close no matter how far");
        let new_object = Bytes::from_static(b"So close no matter if very far");
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        // Validate that the delta instructions get created successfully
        let mut encoded_instructions = Vec::new();
        delta_instructions.write(&mut encoded_instructions).await?;
        let encoded_instructions = Bytes::from(encoded_instructions);
        let (base_object_size, read_till) = decode_header_size(encoded_instructions.as_ref());
        let (new_object_size, _) =
            decode_header_size(encoded_instructions.slice(read_till..).as_ref());
        // Validate that object sizes are encoded in the right order and represent the correct sizes
        assert_eq!(base_object_size as usize, base_object.len());
        assert_eq!(new_object_size as usize, new_object.len());
        Ok(())
    }

    #[fbinit::test]
    async fn test_multi_byte_delta_header() -> Result<()> {
        // Create an arbitrary set of bytes and use that as the base object
        let base_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(1000)
            .collect();
        let base_object = Bytes::from(base_object);
        // Create an arbitrary set of bytes and use that as the new object
        let new_object: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(rand::distributions::Standard)
            .take(1000)
            .collect();
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        // Validate that the delta instructions get created successfully
        let mut encoded_instructions = Vec::new();
        delta_instructions.write(&mut encoded_instructions).await?;
        let encoded_instructions = Bytes::from(encoded_instructions);
        let (base_object_size, read_till) = decode_header_size(encoded_instructions.as_ref());
        let (new_object_size, _) =
            decode_header_size(encoded_instructions.slice(read_till..).as_ref());
        // Validate that object sizes are encoded in the right order and represent the correct sizes
        assert_eq!(base_object_size as usize, base_object.len());
        assert_eq!(new_object_size as usize, new_object.len());
        Ok(())
    }

    #[fbinit::test]
    async fn test_delta_instruction_storage(fb: FacebookInit) -> Result<()> {
        let blobstore: Arc<dyn Blobstore> = Arc::new(Memblob::default());
        let ctx = CoreContext::test_mock(fb);
        // Create a Git tag object pointing to a tree and use it as base object
        let tag = Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Tree,
            name: "TreeTag".into(),
            tagger: None,
            message: "Tag pointing to a tree".into(),
            pgp_signature: None,
        };
        let mut base_object = tag.loose_header().into_vec();
        tag.write_to(base_object.by_ref())?;
        let base_object = Bytes::from(base_object);
        // Create a Git tag object pointing to a blob and use it as the new object
        let tag = Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Blob,
            name: "BlobTag".into(),
            tagger: None,
            message: "Tag pointing to a blob".into(),
            pgp_signature: None,
        };
        let mut new_object = tag.loose_header().into_vec();
        tag.write_to(new_object.by_ref())?;
        let new_object = Bytes::from(new_object);
        let delta_instructions =
            DeltaInstructions::generate(base_object.clone(), new_object.clone(), Algorithm::Myers)?;
        let chunk_prefix =
            DeltaInstructionChunkIdPrefix::new(ONES_CSID, MPath::ROOT, TWOS_CSID, MPath::ROOT);
        let stored_metadata = store_delta_instructions(
            &ctx,
            &blobstore,
            delta_instructions,
            chunk_prefix.clone(),
            Some(4_193_280),
        )
        .await?;
        let fetched_instructions =
            fetch_delta_instructions(&ctx, &blobstore, &chunk_prefix, stored_metadata.chunks)
                .try_fold(
                    BytesMut::with_capacity(stored_metadata.compressed_bytes as usize),
                    |mut acc, bytes| async move {
                        acc.extend_from_slice(bytes.as_ref());
                        anyhow::Ok(acc)
                    },
                )
                .await
                .context("Error in fetching delta instruction bytes from byte stream")?
                .freeze();
        let mut decoded_instructions = Vec::new();
        let mut decoder = ZlibDecoder::new(decoded_instructions);
        decoder.write_all(fetched_instructions.as_ref())?;
        decoded_instructions = decoder.finish()?;
        let decoded_instructions = Bytes::from(decoded_instructions);
        let (base_object_size, first_read) = decode_header_size(decoded_instructions.as_ref());
        let (new_object_size, second_read) =
            decode_header_size(decoded_instructions.slice(first_read..).as_ref());
        // Validate that object sizes are encoded in the right order and represent the correct sizes
        assert_eq!(base_object_size as usize, base_object.len());
        assert_eq!(new_object_size as usize, new_object.len());

        let read_till = first_read + second_read;
        let mut recreated_new_object = Vec::new();
        apply(
            base_object.as_ref(),
            &mut recreated_new_object,
            decoded_instructions.slice(read_till..).as_ref(),
        );
        // Validate that we are able to recreate the Git tag object from
        // the delta-generated bytes
        let object = ObjectRef::from_loose(recreated_new_object.as_ref())
            .with_context(|| {
                format!(
                    "Error in deserialing bytes into Git Object: {}",
                    String::from_utf8_lossy(recreated_new_object.as_ref())
                )
            })?
            .to_owned();
        let output_tag = object
            .try_into_tag()
            .expect("Expected successful conversion into Git Tag");
        // Validate that the Git tag object obtained from the delta-generated bytes is the same
        // as the Tag object used as new_object above
        assert_eq!(tag, output_tag, "Git tag objects do not match");
        Ok(())
    }
}
