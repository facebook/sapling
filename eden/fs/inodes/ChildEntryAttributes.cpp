/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/ChildEntryAttributes.h"

#include <folly/coro/CurrentExecutor.h>

#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/store/ObjectStore.h"

namespace facebook::eden {

folly::coro::Task<EntryAttributes> coFetchEntryAttributesFromVI(
    VirtualInode v,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx) {
  co_await folly::coro::co_reschedule_on_current_executor;
  co_return co_await v.co_getEntryAttributes(
      reqAttrs, sub, store, checkoutTime, ctx);
}

folly::coro::Task<EntryAttributes> coFetchTreeEntryAttributes(
    ObjectId oid,
    mode_t mode,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx) {
  co_await folly::coro::co_reschedule_on_current_executor;
  auto t = co_await store->co_getTree(oid, ctx);
  VirtualInode v{std::move(t), mode};
  co_return co_await v.co_getEntryAttributes(
      reqAttrs, sub, store, checkoutTime, ctx);
}

folly::coro::Task<EntryAttributes> coFetchLoadedInodeEntryAttributes(
    folly::SemiFuture<InodePtr> loadFut,
    EntryAttributeFlags reqAttrs,
    RelativePath sub,
    std::shared_ptr<ObjectStore> store,
    timespec checkoutTime,
    ObjectFetchContextPtr ctx) {
  auto inode = co_await std::move(loadFut);
  VirtualInode v{std::move(inode)};
  co_return co_await v.co_getEntryAttributes(
      reqAttrs, sub, store, checkoutTime, ctx);
}

} // namespace facebook::eden
