/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <string>
#include <vector>

#include <folly/Range.h>
#include <folly/coro/safe/NowTask.h>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/inodes/TreeInode.h"

#include "eden/fs/utils/GlobNodeImpl.h"

namespace facebook::eden {

class EdenMount;
class ServerState;
class Glob;
class GlobParams;
class PrefetchParams;
class ObjectFetchContext;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

class ThriftGlobImpl {
 public:
  explicit ThriftGlobImpl(const GlobParams& params);
  explicit ThriftGlobImpl(
      const PrefetchParams& params,
      bool prefetchOptimizations);

  // DEPRECATED: use co_glob directly. Futures wrapper kept for non-coroutine
  // callers; remove once all callers have migrated.
  ImmediateFuture<std::unique_ptr<Glob>> glob(
      std::shared_ptr<EdenMount> edenMount,
      std::shared_ptr<ServerState> serverState,
      std::vector<std::string> globs,
      const ObjectFetchContextPtr& fetchContext);

  folly::coro::now_task<std::unique_ptr<Glob>> co_glob(
      std::shared_ptr<EdenMount> edenMount,
      std::shared_ptr<ServerState> serverState,
      std::vector<std::string> globs,
      const ObjectFetchContextPtr& fetchContext);

  std::string logString();
  std::string logString(const std::vector<std::string>& globs) const;

 private:
  bool includeDotfiles_{false};
  bool prefetchFiles_{false};
  bool suppressFileList_{false};
  bool wantDtype_{false};
  bool listOnlyFiles_{false};
  std::vector<std::string> rootIds_;
  folly::StringPiece searchRootUser_;
};

folly::coro::now_task<std::vector<BackingStore::GetGlobFilesResult>>
getLocalGlobResults(
    const std::shared_ptr<EdenMount>& edenMount,
    const std::shared_ptr<ServerState>& serverState,
    bool includeDotfiles,
    const std::vector<std::string>& suffixGlobs,
    const std::vector<std::string>& prefixes,
    const TreeInodePtr& rootInode,
    const ObjectFetchContextPtr& context);

} // namespace facebook::eden
