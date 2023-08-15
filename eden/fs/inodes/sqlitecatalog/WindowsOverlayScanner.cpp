/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Helper binary for testing scanning changes in ProjectedFS

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/WinStackTrace.h"

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

  auto loggingConfig = folly::parseLogConfig("eden=DBG2; default:async=true");
  folly::LoggerDB::get().updateConfig(loggingConfig);

  installWindowsExceptionFilter();
  if (argc != 2) {
    fprintf(stderr, "error: missing parameters\n");
    fprintf(stderr, "usage: eden_scanner overlay_path\n");
    return 1;
  }

  auto overlayPath = canonicalPath(argv[1]);
  auto mountPath = canonicalPath(FLAGS_mount_path);

  SqliteInodeCatalog inodeCatalog(
      overlayPath, std::make_shared<NullStructuredLogger>());
  inodeCatalog.initOverlay(/*createIfNonExisting=*/true);
  XLOG(INFO) << "start scanning";
  InodeCatalog::LookupCallback lookup = [](auto, auto) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  inodeCatalog.scanLocalChanges(
      EdenConfig::createTestEdenConfig(), mountPath, true, lookup);
  XLOG(INFO) << "scanning end";

  return 0;
}

#endif // _WIN32
