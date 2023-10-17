/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ChangesetId;
use packfile::pack::DeltaForm;

/// The list of refs that are to be included in or excluded from the pack
#[derive(Debug, Clone)]
pub enum RequestedRefs {
    Include(Vec<String>),
    Exclude(Vec<String>),
}

/// The request parameters used to specify the constraints that need to be
/// honored while generating the input PackfileItem stream
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PackInputStreamRequest {
    /// The refs that are requested to be included/excluded from the pack
    requested_refs: RequestedRefs,
    /// The heads of the references that are present with the client
    have_heads: Vec<ChangesetId>,
    /// Whether the pack input should consist of RefDeltas or only OffsetDeltas
    delta_form: DeltaForm,
    /// The percentage threshold which should be satisfied by the delta to be included
    /// in the pack input stream. The threshold is expressed as percentage of the original (0.0 to 1.0)
    /// uncompressed object size. e.g. If original object size is 100 bytes and the
    /// delta_inclusion_threshold is 0.5, then the delta size should be less than 50 bytes
    delta_inclusion_threshold: f32,
}

impl PackInputStreamRequest {
    pub fn new(
        requested_refs: RequestedRefs,
        have_heads: Vec<ChangesetId>,
        delta_form: DeltaForm,
        delta_inclusion_threshold: f32,
    ) -> Self {
        Self {
            requested_refs,
            have_heads,
            delta_form,
            delta_inclusion_threshold,
        }
    }
}
