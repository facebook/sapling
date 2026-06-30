/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/coro/Task.h>
#include <folly/futures/Future.h>
#include <sys/types.h>
#include <memory>
#include <optional>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/model/EntryAttributeFlags.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class ObjectStore;

/**
 * Per-child fanout helpers for TreeInode::co_getChildrenAttributes() and
 * VirtualInode::co_getChildrenAttributes().
 *
 * These helpers are intentionally narrow: they exist only for the
 * co_getChildrenAttributes fanout path where callers store child work in a
 * vector and await it with collectAllTryRange().
 *
 * They intentionally return folly::coro::Task, rather than now_task, so the
 * call sites can add them to that vector directly without a co_invoke wrapper
 * per child.
 *
 * Task is unsafe with borrowed coroutine state. Keep every async input owned
 * by value; do not add reference parameters or captures here.
 */

/**
 * Fetch EntryAttributes for an already-built VirtualInode.
 */
folly::coro::Task<EntryAttributes> coFetchEntryAttributesFromVI(
    VirtualInode v,
    std::optional<bool> ancestorUnderAcl,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx);

/**
 * Fetch a child tree from the object store and then fetch its
 * EntryAttributes.
 */
folly::coro::Task<EntryAttributes> coFetchTreeEntryAttributes(
    ObjectId oid,
    mode_t mode,
    std::optional<bool> hasACL,
    std::optional<bool> ancestorUnderAcl,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx);

/**
 * Await a pending inode load and then fetch its EntryAttributes.
 */
folly::coro::Task<EntryAttributes> coFetchLoadedInodeEntryAttributes(
    folly::SemiFuture<InodePtr> loadFut,
    std::optional<bool> ancestorUnderAcl,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx);

} // namespace facebook::eden
