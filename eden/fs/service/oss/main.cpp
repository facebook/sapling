/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenMain.h"

#include <folly/logging/xlog.h>

using namespace facebook::eden;

int main(int argc, char** argv) {
  if (!folly::kIsWindows) {
    return runEdenMain(DefaultEdenMain{}, argc, argv);
  } else {
    // TODO(xavierd): on Windows, unhandled exceptions are just simply ignored,
    // giving absolutely no clue to the user as to what is wrong. Let's catch
    // them and display them properly until we have the same infrastructure as
    // on Linux/macOS.
    try {
      return runEdenMain(DefaultEdenMain{}, argc, argv);
    } catch (const std::exception& ex) {
      XLOG(ERR) << folly::exceptionStr(ex);
      return 1;
    }
  }
}
