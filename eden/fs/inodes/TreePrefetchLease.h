/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

/**
 * TreePrefetchLease is a small helper class to track the total number of
 * concurrent tree prefetch operations running in an EdenMount.
 *
 * When TreeInode wants to perform a prefetch it should call
 * EdenMount::tryStartTreePrefetch() to obtain a prefetch lease.  If it obtains
 * a lease it can perform the prefetch, and should hold the TreePrefetchLease
 * object around until the prefetch completes.  When the TreePrefetchLease is
 * destroyed this will inform the EdenMount that the prefetch is complete.
 */
class TreePrefetchLease {
  class TreePrefetchContext : public ObjectFetchContext {
   public:
    TreePrefetchContext(
        std::optional<pid_t> clientPid,
        ObjectFetchContext::Cause cause)
        : clientPid_(clientPid), cause_(cause) {}
    ImportPriority getPriority() const override {
      return kReaddirPrefetchPriority;
    }
    std::optional<pid_t> getClientPid() const override {
      return clientPid_;
    }
    ObjectFetchContext::Cause getCause() const override {
      return cause_;
    }
    const std::unordered_map<std::string, std::string>* FOLLY_NULLABLE
    getRequestInfo() const override {
      return nullptr;
    }

   private:
    std::optional<pid_t> clientPid_ = std::nullopt;
    ObjectFetchContext::Cause cause_;
  };

 public:
  explicit TreePrefetchLease(
      TreeInodePtr inode,
      const ObjectFetchContext& context)
      : inode_{std::move(inode)},
        context_(makeRefPtr<TreePrefetchContext>(
            context.getClientPid(),
            context.getCause())) {}

  ~TreePrefetchLease() {
    release();
  }
  TreePrefetchLease(TreePrefetchLease&& lease) noexcept
      : inode_{std::move(lease.inode_)}, context_(std::move(lease.context_)) {}
  TreePrefetchLease& operator=(TreePrefetchLease&& lease) noexcept {
    if (&lease != this) {
      release();
      inode_ = std::move(lease.inode_);
      context_ = std::move(lease.context_);
    }
    return *this;
  }

  const TreeInodePtr& getTreeInode() const {
    return inode_;
  }

  const ObjectFetchContextPtr& getContext() const {
    return context_;
  }

 private:
  TreePrefetchLease(const TreePrefetchLease& lease) = delete;
  TreePrefetchLease& operator=(const TreePrefetchLease& lease) = delete;

  void release() noexcept;

  TreeInodePtr inode_;

  ObjectFetchContextPtr context_;
};

} // namespace facebook::eden
