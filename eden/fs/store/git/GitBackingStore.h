/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ObjectFetchContext.h"

struct git_oid;
struct git_repository;

namespace facebook::eden {

/**
 * A BackingStore implementation that loads data out of a git repository.
 */
class GitBackingStore final : public BijectiveBackingStore {
 public:
  /**
   * Create a new GitBackingStore.
   */
  explicit GitBackingStore(AbsolutePathPiece repository);
  ~GitBackingStore() override;

  /**
   * Get the repository path.
   *
   * This returns the path to the .git directory itself.
   */
  const char* getPath() const;

  RootId parseRootId(folly::StringPiece rootId) override;
  std::string renderRootId(const RootId& rootId) override;
  ObjectId parseObjectId(folly::StringPiece objectId) override;
  std::string renderObjectId(const ObjectId& objectId) override;

  // TODO(T119221752): Implement for all BackingStore subclasses
  int64_t dropAllPendingRequestsFromQueue() override {
    XLOG(
        WARN,
        "dropAllPendingRequestsFromQueue() is not implemented for GitBackingStore");
    return 0;
  }

 private:
  GitBackingStore(GitBackingStore const&) = delete;
  GitBackingStore& operator=(GitBackingStore const&) = delete;

  ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) override;
  ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& /* objectId */,
      TreeEntryType /* treeEntryType */,
      const ObjectFetchContextPtr& /* context */) override {
    throw std::domain_error("unimplemented");
  }
  folly::SemiFuture<BackingStore::GetTreeResult> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<BackingStore::GetTreeAuxResult> getTreeAuxData(
      const ObjectId& /*id*/,
      const ObjectFetchContextPtr& /*context*/) override;
  folly::SemiFuture<BackingStore::GetBlobResult> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::coro::Task<BackingStore::GetBlobResult> co_getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<BackingStore::GetBlobAuxResult> getBlobAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  ImmediateFuture<GetGlobFilesResult> getGlobFiles(
      const RootId& id,
      const std::vector<std::string>& globs,
      const std::vector<std::string>& prefixes) override;

  TreePtr getTreeImpl(const ObjectId& id);
  BlobPtr getBlobImpl(const ObjectId& id);

  static git_oid root2Oid(const RootId& rootId);

  static git_oid hash2Oid(const ObjectId& hash);
  static ObjectId oid2Hash(const git_oid* oid);

  git_repository* repo_{nullptr};
};

} // namespace facebook::eden
