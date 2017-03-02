/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Format.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileInode.h"

/**
 * Check that a FileInode has the expected contents and permissions.
 */
#define EXPECT_FILE_INODE(fileInode, expectedData, expectedPerms)         \
  do {                                                                    \
    auto fileDataForCheck = (fileInode)->getOrLoadData();                 \
    fileDataForCheck->materializeForRead(O_RDONLY);                       \
    EXPECT_EQ(                                                            \
        (expectedData), folly::StringPiece{fileDataForCheck->readAll()}); \
    EXPECT_EQ(                                                            \
        folly::sformat("{:#o}", (expectedPerms)),                         \
        folly::sformat("{:#o}", (fileInode)->getPermissions()));          \
  } while (0)
