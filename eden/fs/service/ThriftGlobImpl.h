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
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/RefPtr.h"

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
  explicit ThriftGlobImpl(const PrefetchParams& params);

  ImmediateFuture<std::unique_ptr<Glob>> glob(
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
  std::vector<std::string> rootHashes_;
  folly::StringPiece searchRootUser_;
};

} // namespace facebook::eden
