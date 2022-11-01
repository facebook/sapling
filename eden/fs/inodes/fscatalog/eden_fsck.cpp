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
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>

#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog/OverlayChecker.h"

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

DEFINE_bool(
    dry_run,
    false,
    "Only report errors, without attempting to fix any problems");

using namespace facebook::eden;

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  if (argc != 2) {
    fprintf(stderr, "error: no overlay path\n");
    fprintf(stderr, "usage: eden_store_util COMMAND\n");
    return EX_USAGE;
  }

  std::optional<FileContentStore> fileContentStore;
  std::optional<FsInodeCatalog> fsInodeCatalog;
  std::optional<InodeNumber> nextInodeNumber;
  auto overlayPath = normalizeBestEffort(argv[1]);
  try {
    fileContentStore.emplace(overlayPath);
    fsInodeCatalog.emplace(&fileContentStore.value());
    nextInodeNumber =
        fsInodeCatalog->initOverlay(/*createIfNonExisting=*/false);
  } catch (std::exception& ex) {
    XLOG(ERR) << "unable to open overlay: " << folly::exceptionStr(ex);
    return 1;
  }

  if (!nextInodeNumber.has_value()) {
    XLOG(INFO) << "Overlay was shut down uncleanly";
  }

  OverlayChecker::LookupCallback lookup = [](auto&&) {
    return makeImmediateFuture<OverlayChecker::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      &fsInodeCatalog.value(),
      &fileContentStore.value(),
      nextInodeNumber,
      lookup);
  checker.scanForErrors();
  if (FLAGS_dry_run) {
    checker.logErrors();
    fsInodeCatalog->close(nextInodeNumber);
  } else {
    checker.repairErrors();
    fsInodeCatalog->close(checker.getNextInodeNumber());
  }
  return 0;
}
