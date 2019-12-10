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

            // It is too costly to have trees be deleted by automatic
            // background GC when there are programs that cause every
            // tree in the repo to be fetched. Make ephemeral when GC
            // is smarter and when Eden can more efficiently read from
            // the hg cache.  This would also be better if programs
            // weren't scanning the entire repo for filenames, causing
            // every tree to be loaded.
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
                                       "hgcommit2tree"},

            LocalStore::KeySpaceRecord{LocalStore::BlobSizeFamily,
                                       LocalStore::Persistence::Ephemeral,
                                       "blobsize"}};

} // namespace eden
} // namespace facebook
