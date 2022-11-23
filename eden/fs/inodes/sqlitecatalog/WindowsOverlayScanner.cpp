/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Helper binary for testing scanning changes in ProjectedFS

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/fscatalog/OverlayChecker.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/WinStackTrace.h"

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

DEFINE_string(
    mount_path,
    "C:\\open\\fbsource",
    "Only report errors, without attempting to fix any problems");

using namespace facebook::eden;

#ifndef _WIN32

int main(int, char**) {
  fprintf(stderr, "this tool only works on Windows");
  return 1;
}

#else

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  installWindowsExceptionFilter();
  if (argc != 2) {
    fprintf(stderr, "error: missing parameters\n");
    fprintf(stderr, "usage: eden_scanner overlay_path\n");
    return 1;
  }

  auto overlayPath = canonicalPath(argv[1]);
  auto mountPath = canonicalPath(FLAGS_mount_path);

  SqliteInodeCatalog inodeCatalog(overlayPath);
  inodeCatalog.initOverlay(true);
  XLOG(INFO) << "start scanning";
  SqliteInodeCatalog::LookupCallback lookup = [](auto) {
    return makeImmediateFuture<SqliteInodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  inodeCatalog.scanLocalChanges(
      EdenConfig::createTestEdenConfig(), mountPath, lookup);
  XLOG(INFO) << "scanning end";

  return 0;
}

#endif // _WIN32
