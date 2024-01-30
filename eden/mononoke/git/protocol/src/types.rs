/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;
use futures::stream::BoxStream;
use gix_hash::ObjectId;
use mononoke_types::ChangesetId;
use packfile::pack::DeltaForm;
use packfile::types::PackfileItem;

/// Enum defining the type of data associated with a ref target
pub enum RefTarget {
    /// The target is a plain Git object
    Plain(ObjectId),
    /// The target is a Git object with associated metadata
    WithMetadata(ObjectId, String),
}

impl RefTarget {
    pub fn id(&self) -> &ObjectId {
        match self {
            RefTarget::Plain(oid) | RefTarget::WithMetadata(oid, _) => oid,
        }
    }

    pub fn into_commit(self) -> ObjectId {
        match self {
            RefTarget::Plain(oid) | RefTarget::WithMetadata(oid, _) => oid,
        }
    }
}

/// The set of symrefs that are to be included in or excluded from the pack
#[derive(Debug, Clone, Copy)]
pub enum RequestedSymrefs {
    /// Only include the HEAD symref in the pack/bundle
    IncludeHead(SymrefFormat),
    /// Incldue all known symrefs in the pack/bundle
    IncludeAll(SymrefFormat),
    /// Exclude all known symrefs from the pack/bundle
    ExcludeAll,
}

/// The format in which the symrefs need to be included in the pack
#[derive(Debug, Clone, Copy)]
pub enum SymrefFormat {
    /// Include the symref along with the ref that it points to, e.g.
    /// object_id_here HEAD symref-target:refs/heads/master
    NameWithTarget,
    /// Only include the symref name, e.g. object_id_here HEAD
    NameOnly,
}

/// The set of refs that are to be included in or excluded from the pack
#[derive(Debug, Clone)]
pub enum RequestedRefs {
    /// Include the following refs with values known by the server
    Included(HashSet<String>),
    /// Include the following refs with values provided by the caller
    IncludedWithValue(HashMap<String, ChangesetId>),
    /// Exclude the following refs
    Excluded(HashSet<String>),
}

impl RequestedRefs {
    pub fn all() -> Self {
        RequestedRefs::Excluded(HashSet::new())
    }

    pub fn none() -> Self {
        RequestedRefs::Included(HashSet::new())
    }
}

/// Enum defining how annotated tags should be included as a ref
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagInclusion {
    /// Peel the tag and map it to the underlying Git commit
    Peeled,
    /// Include the tag as-is without peeling and map it to
    /// the annotated Git tag object
    AsIs,
    /// Include the tag as-is without peeling but reference the
    /// peeled target of the tag
    WithTarget,
}

/// Enum defining whether a delta should be included in the pack
/// and if so, what kind of delta should be used
#[derive(Debug, Clone, Copy)]
pub enum DeltaInclusion {
    /// Include deltas with the provided form and inclusion threshold
    Include {
        /// Whether the pack input should consist of RefDeltas or only OffsetDeltas
        form: DeltaForm,
        /// The percentage threshold which should be satisfied by the delta to be included
        /// in the pack input stream. The threshold is expressed as percentage of the original (0.0 to 1.0)
        /// uncompressed object size. e.g. If original object size is 100 bytes and the
        /// delta_inclusion_threshold is 0.5, then the delta size should be less than 50 bytes
        inclusion_threshold: f32,
    },
    /// Do not include deltas
    Exclude,
}

impl DeltaInclusion {
    /// The standard delta inclusion setting used in most places
    /// in Mononoke
    pub fn standard() -> Self {
        DeltaInclusion::Include {
            form: DeltaForm::RefAndOffset,
            inclusion_threshold: 0.6,
        }
    }
}

impl DeltaInclusion {
    pub fn include_only_offset_deltas(&self) -> bool {
        match self {
            DeltaInclusion::Include { form, .. } => form == &DeltaForm::OnlyOffset,
            DeltaInclusion::Exclude => false,
        }
    }
}

/// Enum defining how packfile items for raw git objects be fetched
#[derive(clap::ValueEnum, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackfileItemInclusion {
    // Generate the packfile item for raw git object regardless of whether
    // it already exists or not. Do not store the generated packfile item.
    #[default]
    Generate,
    // Fetch the stored packfile item for the raw git object. If it doesn't
    // exist, error out
    FetchOnly,
    // If the packfile item for the raw git object already exists, use it. If
    // it doesn't exist, generate it and store it
    FetchAndStore,
}

/// The request parameters used to specify the constraints that need to be
/// honored while generating the input PackfileItem stream
#[derive(Debug, Clone)]
pub struct PackItemStreamRequest {
    /// The symrefs that are requested to be included/excluded from the pack
    pub requested_symrefs: RequestedSymrefs,
    /// The refs that are requested to be included/excluded from the pack
    pub requested_refs: RequestedRefs,
    /// The heads of the references that are present with the client
    pub have_heads: Vec<ChangesetId>,
    /// The type of delta that should be included in the pack, if any
    pub delta_inclusion: DeltaInclusion,
    /// How annotated tags should be included in the pack
    pub tag_inclusion: TagInclusion,
    /// How packfile items for raw git objects should be fetched
    pub packfile_item_inclusion: PackfileItemInclusion,
}

impl PackItemStreamRequest {
    pub fn new(
        requested_symrefs: RequestedSymrefs,
        requested_refs: RequestedRefs,
        have_heads: Vec<ChangesetId>,
        delta_inclusion: DeltaInclusion,
        tag_inclusion: TagInclusion,
        packfile_item_inclusion: PackfileItemInclusion,
    ) -> Self {
        Self {
            requested_symrefs,
            requested_refs,
            have_heads,
            delta_inclusion,
            tag_inclusion,
            packfile_item_inclusion,
        }
    }

    pub fn full_repo(
        delta_inclusion: DeltaInclusion,
        tag_inclusion: TagInclusion,
        packfile_item_inclusion: PackfileItemInclusion,
    ) -> Self {
        Self {
            requested_symrefs: RequestedSymrefs::IncludeHead(SymrefFormat::NameOnly),
            requested_refs: RequestedRefs::Excluded(HashSet::new()),
            have_heads: vec![],
            delta_inclusion,
            tag_inclusion,
            packfile_item_inclusion,
        }
    }
}

/// Struct representing the packfile item response generated for the
/// given range of commits
pub struct PackItemStreamResponse<'a> {
    /// The stream of packfile items that were generated for the given range of commits
    pub items: BoxStream<'a, Result<PackfileItem>>,
    /// The number of packfile items that were generated for the given range of commits
    pub num_items: usize,
    /// The set of refs mapped to their Git commit ID or tag ID that are included in the
    /// generated stream of packfile items along with optional metadata for the mapping
    pub included_refs: HashMap<String, RefTarget>,
}

impl<'a> PackItemStreamResponse<'a> {
    pub fn new(
        items: BoxStream<'a, Result<PackfileItem>>,
        num_items: usize,
        included_refs: HashMap<String, RefTarget>,
    ) -> Self {
        Self {
            items,
            num_items,
            included_refs,
        }
    }
}
