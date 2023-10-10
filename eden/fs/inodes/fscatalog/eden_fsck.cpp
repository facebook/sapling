/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <sysexits.h>
#include <optional>

#include <folly/Exception.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"

DEFINE_bool(
    dry_run,
    false,
    "Only report errors, without attempting to fix any problems");

DEFINE_bool(
    force,
    false,
    "Force fsck to scan for errors even on checkouts that appear to currently be mounted.  It will not attempt to fix any problems, but will only scan and report possible issues");

using namespace facebook::eden;

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  if (argc != 2) {
    fprintf(stderr, "error: no overlay path provided\n");
    fprintf(stderr, "usage: eden_fsck PATH [ARGS]\n");
    return EX_USAGE;
  }

  auto loggingConfig = folly::parseLogConfig("eden=DBG2; default:async=true");
  folly::LoggerDB::get().updateConfig(loggingConfig);

  std::optional<FsFileContentStore> fileContentStore;
  std::optional<FsInodeCatalog> fsInodeCatalog;
  std::optional<InodeNumber> nextInodeNumber;
  auto overlayPath = normalizeBestEffort(argv[1]);
  try {
    fileContentStore.emplace(overlayPath);
    fsInodeCatalog.emplace(&fileContentStore.value());
    nextInodeNumber = fsInodeCatalog->initOverlay(
        /*createIfNonExisting=*/false, /*bypassLockFile=*/FLAGS_force);
  } catch (std::exception& ex) {
    XLOG(ERR) << "unable to open overlay: " << folly::exceptionStr(ex);
    return 1;
  }

  if (!nextInodeNumber.has_value()) {
    XLOG(INFO) << "Overlay was shut down uncleanly";
  }

  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      &fsInodeCatalog.value(),
      &fileContentStore.value(),
      nextInodeNumber,
      lookup);
  checker.scanForErrors();
  if (FLAGS_dry_run || FLAGS_force) {
    checker.logErrors();
    fsInodeCatalog->close(nextInodeNumber);
  } else {
    checker.repairErrors();
    fsInodeCatalog->close(checker.getNextInodeNumber());
  }
  return 0;
}
