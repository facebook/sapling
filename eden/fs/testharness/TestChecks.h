/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/format.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <folly/portability/GTest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/store/ObjectFetchContext.h"

/**
 * Check that a FileInode has the expected contents and permissions.
 */
#ifndef _WIN32
#define EXPECT_FILE_INODE(fileInode, expectedData, expectedPerms)              \
  do {                                                                         \
    EXPECT_EQ(                                                                 \
        expectedData,                                                          \
        folly::StringPiece{(fileInode)                                         \
                               ->readAll(ObjectFetchContext::getNullContext()) \
                               .get(std::chrono::seconds(20))})                \
        << " for inode path " << (fileInode)->getLogPath();                    \
    EXPECT_EQ(                                                                 \
        fmt::format("{:#o}", (expectedPerms)),                                 \
        fmt::format("{:#o}", (fileInode)->getPermissions()))                   \
        << " for inode path " << (fileInode)->getLogPath();                    \
  } while (0)
#else
#define EXPECT_FILE_INODE(fileInode, expectedData, expectedPerms)              \
  do {                                                                         \
    EXPECT_EQ(                                                                 \
        expectedData,                                                          \
        folly::StringPiece{(fileInode)                                         \
                               ->readAll(ObjectFetchContext::getNullContext()) \
                               .get(std::chrono::seconds(20))})                \
        << " for inode path " << (fileInode)->getLogPath();                    \
  } while (0)
#endif
