/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/HgBinary.h"
#include <folly/Range.h>
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
AbsolutePath findAndConfigureHgBinary() {
  AbsolutePath hgBinary = findHgBinary();

  // Have HgImporter use the test hg binary
  FLAGS_hgPath = hgBinary.value();

  return hgBinary;
}

AbsolutePath findHgBinary() {
  auto hgPath = getenv("EDEN_HG_BINARY");
  if (hgPath) {
    return realpath(hgPath);
  }

  // Search through $PATH if $EDEN_HG_BINARY was not explicitly specified
  auto pathPtr = getenv("PATH");
  if (!pathPtr) {
    throw std::runtime_error("unable to find hg command: no PATH");
  }
  folly::StringPiece pathEnv{pathPtr};
  std::vector<std::string> pathEnvParts;
  folly::split(":", pathEnv, pathEnvParts);

  for (const auto& dir : pathEnvParts) {
    for (const auto& name : {"hg.real", "hg.real.exe", "hg", "hg.exe"}) {
      auto exePath = folly::to<std::string>(dir, "/", name);
      XLOG(DBG5) << "Checking for hg at " << exePath;
      if (access(exePath.c_str(), X_OK) == 0) {
        return realpath(exePath);
      }
    }
  }

  throw std::runtime_error("unable to find hg in PATH");
}
} // namespace facebook::eden
