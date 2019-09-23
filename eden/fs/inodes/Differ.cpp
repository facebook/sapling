/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/Differ.h"
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {
void ThriftStatusCallback::ignoredFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::IGNORED);
}

void ThriftStatusCallback::untrackedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::ADDED);
}

void ThriftStatusCallback::removedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::REMOVED);
}

void ThriftStatusCallback::modifiedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::MODIFIED);
}

void ThriftStatusCallback::diffError(
    RelativePathPiece path,
    const folly::exception_wrapper& ew) {
  XLOG(WARNING) << "error computing status data for " << path << ": "
                << folly::exceptionStr(ew);
  data_.wlock()->errors.emplace(
      path.stringPiece().str(), folly::exceptionStr(ew));
}

/**
 * Extract the ScmStatus object from this callback.
 *
 * This method should be called no more than once, as this destructively
 * moves the results out of the callback.  It should only be invoked after
 * the diff operation has completed.
 */
ScmStatus ThriftStatusCallback::extractStatus() {
  auto data = data_.wlock();
  return std::move(*data);
}

char scmStatusCodeChar(ScmFileStatus code) {
  switch (code) {
    case ScmFileStatus::ADDED:
      return 'A';
    case ScmFileStatus::MODIFIED:
      return 'M';
    case ScmFileStatus::REMOVED:
      return 'R';
    case ScmFileStatus::IGNORED:
      return 'I';
  }
  throw std::runtime_error(folly::to<std::string>(
      "Unrecognized ScmFileStatus: ",
      static_cast<typename std::underlying_type<ScmFileStatus>::type>(code)));
}

std::ostream& operator<<(std::ostream& os, const ScmStatus& status) {
  os << "{";
  for (const auto& pair : status.get_entries()) {
    os << scmStatusCodeChar(pair.second) << " " << pair.first << "; ";
  }
  os << "}";
  return os;
}

folly::Future<std::unique_ptr<ScmStatus>>
diffMountForStatus(const EdenMount& mount, Hash commitHash, bool listIgnored) {
  auto callback = std::make_unique<ThriftStatusCallback>();
  auto callbackPtr = callback.get();
  return mount.diff(callbackPtr, commitHash, listIgnored)
      .thenValue([callback = std::move(callback)](auto&&) {
        return std::make_unique<ScmStatus>(callback->extractStatus());
      });
}

} // namespace eden
} // namespace facebook
