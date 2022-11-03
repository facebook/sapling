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
  return runEdenMain(DefaultEdenMain{}, argc, argv);
}
