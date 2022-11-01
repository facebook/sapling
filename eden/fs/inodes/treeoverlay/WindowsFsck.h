/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef _WIN32

#include "eden/fs/inodes/treeoverlay/SqliteInodeCatalog.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class EdenConfig;

/**
 * Walk the directory hierarchy for the given `mountPath` and fix the
 * divergence in our overlay.
 *
 * On Windows, this is necessary as users can still make changes to the mount
 * point when EdenFS is not running, causing overlay to diverge from the state
 * of the filesystem.
 *
 * In this function, we will deal with several different ProjectedFS file
 * states, and we rely on these relationships to correctly infer the
 * divergences. Specifically, ProjectedFS entries can be in:
 *
 * - Full: this state refers to entries originally created by users, and users
 *   are able to modify their content freely when EdenFS is not running. It is
 *   impossible to have entires in state other than Full under a Full
 *   directory.
 * - DirtyPlaceholder: this state can only be seen in directories. This
 *   indicates the directory was originally served from EdenFS but got modified
 *   by users either by adding or removing entries. Users are only able to
 *   remove entries from DirtyPlaceholder directory when EdenFS is not running.
 * - Placeholder: this state refers to entries that were originally provided
 *   from EdenFS. Users cannot modify its content at all when EdenFS is not
 *   running.
 * - Tombstone: this state refers to entries that were deleted by users when
 *   EdenFS was running. It will only appear in directory walks when EdenFS is
 *   not running. It should be ignored.
 *
 * See also: https://docs.microsoft.com/en-us/windows/win32/projfs/cache-state
 *
 */
void windowsFsckScanLocalChanges(
    std::shared_ptr<const EdenConfig> config,
    SqliteInodeCatalog& overlay,
    AbsolutePathPiece mountPath,
    SqliteInodeCatalog::LookupCallback& callback);

} // namespace facebook::eden

#endif
