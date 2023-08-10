/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/InodeCatalogOptions.h"

namespace facebook::eden {

const InodeCatalogOptions::NameTable InodeCatalogOptions::table{
    {INODE_CATALOG_DEFAULT, "INODE_CATALOG_DEFAULT"},
    {INODE_CATALOG_UNSAFE_IN_MEMORY, "INODE_CATALOG_UNSAFE_IN_MEMORY"},
    {INODE_CATALOG_SYNCHRONOUS_OFF, "INODE_CATALOG_SYNCHRONOUS_OFF"},
    {INODE_CATALOG_BUFFERED, "INODE_CATALOG_BUFFERED"},
};

} // namespace facebook::eden
