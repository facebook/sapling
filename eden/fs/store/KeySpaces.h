/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

static constexpr std::
    array<LocalStore::KeySpaceRecord, LocalStore::KeySpace::End>
        kKeySpaceRecords = {
            LocalStore::KeySpaceRecord{LocalStore::BlobFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "blob"},
            LocalStore::KeySpaceRecord{
                LocalStore::BlobMetaDataFamily,
                LocalStore::Persistence::Ephemeral,
                "blobmeta",
            },

            // If the trees were imported from a flatmanifest, we cannot delete
            // them. See test_contents_are_the_same_if_handle_is_held_open when
            // running against a flatmanifest repository.
            LocalStore::KeySpaceRecord{LocalStore::TreeFamily,
                                       LocalStore::Persistence::Persistent,
                                       "tree"},

            // Proxy hashes are required to fetch objects from hg from a hash.
            // Deleting them breaks re-importing after an inode is unloaded.
            LocalStore::KeySpaceRecord{LocalStore::HgProxyHashFamily,
                                       LocalStore::Persistence::Persistent,
                                       "hgproxyhash"},

            LocalStore::KeySpaceRecord{LocalStore::HgCommitToTreeFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "hgcommit2tree"}};

} // namespace eden
} // namespace facebook
