/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::Result;
use futures::Stream;
use gix_hash::ObjectId;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::pack::DeltaForm;
use crate::pack::PackfileWriter;
use crate::types::PackfileItem;

/// The message/comment associated with the pre-requisite objects
const BUNDLE_PREREQ_MSG: &str = "bundled object";

/// Enum representing the supported bundle versions
/// Currently only version 2 is supported.
pub enum BundleVersion {
    V2,
}

impl Display for BundleVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleVersion::V2 => f.write_str("# v2 git bundle\n"),
        }
    }
}

/// Struct responsible for writing a Git bundle with format https://git-scm.com/docs/bundle-format
/// to the underlying writer.
pub struct BundleWriter<T>
where
    T: AsyncWrite + Unpin,
{
    /// List of objects that are NOT included in the bundle but are
    /// required to be present for unbundling to work.
    pub prereqs: Vec<ObjectId>,
    /// The version of bundle format
    pub version: BundleVersion,
    /// List of ref-names with the commits IDs that they point to along with
    /// optional metadata associated to the refs
    pub refs: Vec<(String, ObjectId)>,
    /// Packfile writer created over the underlying raw writer
    pub pack_writer: PackfileWriter<T>,
    bytes_written: usize,
}

impl<T: AsyncWrite + Unpin> BundleWriter<T> {
    /// Create a new BundleWriter instance with the header of the bundle written to the
    /// underlying writer.
    pub async fn new_with_header(
        mut writer: T,
        refs: Vec<(String, ObjectId)>,
        prereqs: Vec<ObjectId>,
        num_objects: u32,
        concurrency: usize,
        delta_form: DeltaForm,
    ) -> Result<Self> {
        let mut bytes_written = 0;
        // Append the bundle header
        let bundle_header = format!("{}", BundleVersion::V2);
        writer.write_all(bundle_header.as_bytes()).await?;
        bytes_written += bundle_header.len();

        // Append the pre-requisite objects, if present
        for prereq in prereqs.iter() {
            let line = format!("-{} {}\n", prereq, BUNDLE_PREREQ_MSG);
            writer.write_all(line.as_bytes()).await?;
            bytes_written += line.len();
        }
        // Append the refs
        for (ref_name, id) in &refs {
            let line = format!("{} {}\n", id, ref_name);
            writer.write_all(line.as_bytes()).await?;
            bytes_written += line.len();
        }
        // Newline before starting packfile
        writer.write_all(b"\n").await?;
        bytes_written += 1;
        let pack_writer = PackfileWriter::new(writer, num_objects, concurrency, delta_form);
        Ok(Self {
            version: BundleVersion::V2,
            refs,
            prereqs,
            pack_writer,
            bytes_written,
        })
    }

    /// Write the stream of input items to the bundle
    pub async fn write(
        &mut self,
        objects_stream: impl Stream<Item = Result<PackfileItem>>,
    ) -> Result<()> {
        self.pack_writer.write(objects_stream).await
    }

    pub fn bytes_written(&self) -> usize {
        self.bytes_written + self.pack_writer.size as usize
    }

    /// Finish the bundle and flush it to the underlying writer
    /// returning the checksum of the written packfile
    pub async fn finish(&mut self) -> Result<ObjectId> {
        self.pack_writer.finish().await
    }

    /// Consumes the instance after writing the bundle and returns
    /// the underlying raw writer.
    pub fn into_write(self) -> T {
        self.pack_writer.into_write()
    }
}
