/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/TempFile.h"

#include <folly/logging/xlog.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(TempFile, mktemp) {
  // This mainly just verifies that makeTempFile() and makeTempDir() succeeds
  auto tempfile = makeTempFile();
  XLOG(INFO) << "temporary file is " << tempfile.path();
  auto tempdir = makeTempDir();
  XLOG(INFO) << "temporary dir is " << tempfile.path();
}
