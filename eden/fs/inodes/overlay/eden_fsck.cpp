/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#include <gflags/gflags.h>

#include "eden/fs/inodes/overlay/FsOverlay.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"

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

  std::optional<FsOverlay> fsOverlay;
  std::optional<InodeNumber> nextInodeNumber;
  auto overlayPath = normalizeBestEffort(argv[1]);
  try {
    fsOverlay.emplace(overlayPath);
    nextInodeNumber = fsOverlay->initOverlay(/*createIfNonExisting=*/false);
  } catch (std::exception& ex) {
    XLOG(ERR) << "unable to open overlay: " << folly::exceptionStr(ex);
    return 1;
  }

  if (!nextInodeNumber.has_value()) {
    XLOG(INFO) << "Overlay was shut down uncleanly";
  }

  OverlayChecker checker(&fsOverlay.value(), nextInodeNumber);
  checker.scanForErrors();
  if (FLAGS_dry_run) {
    checker.logErrors();
    fsOverlay->close(nextInodeNumber);
  } else {
    checker.repairErrors();
    fsOverlay->close(checker.getNextInodeNumber());
  }
  return 0;
}
