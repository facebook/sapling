/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
