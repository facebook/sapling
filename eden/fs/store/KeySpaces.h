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

            LocalStore::KeySpaceRecord{LocalStore::TreeFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "tree"},

            // Proxy hashes are required to fetch objects from hg from a hash.
            // Deleting them breaks re-importing after an inode is unloaded.
            LocalStore::KeySpaceRecord{LocalStore::HgProxyHashFamily,
                                       LocalStore::Persistence::Persistent,
                                       "hgproxyhash"},

            LocalStore::KeySpaceRecord{LocalStore::HgCommitToTreeFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "hgcommit2tree"},

            LocalStore::KeySpaceRecord{LocalStore::BlobSizeFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "blobsize"}};

} // namespace eden
} // namespace facebook
