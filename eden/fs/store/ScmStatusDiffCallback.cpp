/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {
void ScmStatusDiffCallback::ignoredFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::IGNORED);
}

void ScmStatusDiffCallback::addedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::ADDED);
}

void ScmStatusDiffCallback::removedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::REMOVED);
}

void ScmStatusDiffCallback::modifiedFile(RelativePathPiece path) {
  data_.wlock()->entries.emplace(
      path.stringPiece().str(), ScmFileStatus::MODIFIED);
}

void ScmStatusDiffCallback::diffError(
    RelativePathPiece path,
    const folly::exception_wrapper& ew) {
  XLOG(WARNING) << "error computing status data for " << path << ": "
                << folly::exceptionStr(ew);
  data_.wlock()->errors.emplace(
      path.stringPiece().str(), folly::exceptionStr(ew).toStdString());
}

/**
 * Extract the ScmStatus object from this callback.
 *
 * This method should be called no more than once, as this destructively
 * moves the results out of the callback.  It should only be invoked after
 * the diff operation has completed.
 */
ScmStatus ScmStatusDiffCallback::extractStatus() {
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

} // namespace eden
} // namespace facebook
