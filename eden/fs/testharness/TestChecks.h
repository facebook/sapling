/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Format.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/FileInode.h"

/**
 * Check that a FileInode has the expected contents and permissions.
 */
#define EXPECT_FILE_INODE(fileInode, expectedData, expectedPerms)  \
  do {                                                             \
    EXPECT_EQ(                                                     \
        expectedData,                                              \
        folly::StringPiece{                                        \
            (fileInode)->readAll().get(std::chrono::seconds(20))}) \
        << " for inode path " << (fileInode)->getLogPath();        \
    EXPECT_EQ(                                                     \
        folly::sformat("{:#o}", (expectedPerms)),                  \
        folly::sformat("{:#o}", (fileInode)->getPermissions()))    \
        << " for inode path " << (fileInode)->getLogPath();        \
  } while (0)
