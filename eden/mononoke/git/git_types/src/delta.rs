/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Uses https://git-scm.com/docs/pack-format#_deltified_representation as source

use std::ops::Range;

use anyhow::Result;
use bytes::Bytes;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

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

/// Individual instruction for constructing a part of a
/// new object based on a base object
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
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

/// List of instructions which when applied in order form a
/// complete new object based on delta of a base object
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DeltaInstructions {
    base_object_size: u64,
    new_object_size: u64,
    instructions: Vec<DeltaInstruction>,
}

#[allow(dead_code)]
impl DeltaInstructions {
    pub fn new(base_object_size: u64, new_object_size: u64) -> Self {
        Self {
            base_object_size,
            new_object_size,
            instructions: Vec::new(),
        }
    }

    pub async fn write(&self, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        // Write the size of the base object
        write_size(self.base_object_size, out).await?;
        // Write the size of the new object
        write_size(self.new_object_size, out).await?;
        // Write the delta instructions in order
        for instruction in self.instructions.iter() {
            instruction.write(out).await?;
        }
        Ok(())
    }
}

/// Write the size "size" using the size encoding scheme used by Git
/// The encoding scheme is one of variable length where the bytes are written
/// in little-endian order. Only the lower 7 bits of each byte are used to represent
/// the size data and the 8th bit is used to represent continuation.
async fn write_size(size_to_write: u64, out: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
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
    use super::*;

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
}
