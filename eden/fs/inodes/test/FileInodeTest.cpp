/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/FileInode.h"

#include <folly/Format.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <chrono>

#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::chrono::duration_cast;

std::ostream& operator<<(std::ostream& os, const timespec& ts) {
  os << folly::sformat("{}.{:09d}", ts.tv_sec, ts.tv_nsec);
  return os;
}

namespace std {
namespace chrono {
std::ostream& operator<<(
    std::ostream& os,
    const std::chrono::system_clock::time_point& tp) {
  auto duration = tp.time_since_epoch();
  auto secs = duration_cast<std::chrono::seconds>(duration);
  auto nsecs = duration_cast<std::chrono::nanoseconds>(duration - secs);
  os << folly::sformat("{}.{:09d}", secs.count(), nsecs.count());
  return os;
}
} // namespace chrono
} // namespace std

template <typename Clock = std::chrono::system_clock>
typename Clock::time_point timespecToTimePoint(const timespec& ts) {
  auto duration =
      std::chrono::seconds{ts.tv_sec} + std::chrono::nanoseconds{ts.tv_nsec};
  return typename Clock::time_point{duration};
}

/*
 * Helper functions for comparing timespec structs from file attributes
 * against C++11-style time_point objects.
 */
bool operator<(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) < tp;
}
bool operator<=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) <= tp;
}
bool operator>(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) > tp;
}
bool operator>=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) >= tp;
}
bool operator!=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) != tp;
}
bool operator==(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return timespecToTimePoint(ts) == tp;
}

namespace {

fusell::Dispatcher::Attr getFileAttr(const FileInodePtr& inode) {
  auto attrFuture = inode->getattr();
  // We unfortunately can't use an ASSERT_* check here, since it tries
  // to return from the function normally, rather than throwing.
  if (!attrFuture.isReady()) {
    // Use ADD_FAILURE() so that any SCOPED_TRACE() data will be reported,
    // then throw an exception.
    ADD_FAILURE() << "getattr() future is not ready";
    throw std::runtime_error("getattr future is not ready");
  }
  return attrFuture.get();
}

fusell::Dispatcher::Attr setFileAttr(
    const FileInodePtr& inode,
    const struct stat& desired,
    int setattrMask) {
  auto attrFuture = inode->setattr(desired, setattrMask);
  if (!attrFuture.isReady()) {
    ADD_FAILURE() << "setattr() future is not ready";
    throw std::runtime_error("setattr future is not ready");
  }
  return attrFuture.get();
}

/**
 * Helper function used by BASIC_ATTR_CHECKS()
 */
void basicAttrChecks(
    const FileInodePtr& inode,
    const fusell::Dispatcher::Attr& attr) {
  EXPECT_EQ(inode->getNodeId(), attr.st.st_ino);
  EXPECT_EQ(1, attr.st.st_nlink);
  EXPECT_EQ(inode->getMount()->getUid(), attr.st.st_uid);
  EXPECT_EQ(inode->getMount()->getGid(), attr.st.st_gid);
  EXPECT_EQ(0, attr.st.st_rdev);
  EXPECT_GT(attr.st.st_atime, 0);
  EXPECT_GT(attr.st.st_mtime, 0);
  EXPECT_GT(attr.st.st_ctime, 0);
}

/**
 * Helper function used by BASIC_ATTR_CHECKS()
 */
fusell::Dispatcher::Attr basicAttrChecks(const FileInodePtr& inode) {
  auto attr = getFileAttr(inode);
  basicAttrChecks(inode, attr);
  return attr;
}

/**
 * Run some basic sanity checks on an inode's attributes.
 *
 * This can be invoked with either a two arguments (an inode and attributes),
 * or with just a single argument (just the inode).  If only one argument is
 * supplied the attributes will be retrieved by calling getattr() on the inode.
 *
 * This checks several fixed invariants:
 * - The inode number reported in the attributes should match the input inode's
 *   number.
 * - The UID and GID should match the EdenMount's user and group IDs.
 * - The link count should always be 1.
 * - The timestamps should be greater than 0.
 */
#define BASIC_ATTR_CHECKS(inode, ...)                                         \
  ({                                                                          \
    SCOPED_TRACE(                                                             \
        folly::to<std::string>("Originally from ", __FILE__, ":", __LINE__)); \
    basicAttrChecks(inode, ##__VA_ARGS__);                                    \
  })
} // namespace

class FileInodeTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.setFiles({
        {"dir/a.txt", "This is a.txt.\n"},
    });
    mount_.initialize(builder);
  }

  TestMount mount_;
};

TEST_F(FileInodeTest, getattrFromBlob) {
  auto inode = mount_.getFileInode("dir/a.txt");
  auto attr = getFileAttr(inode);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(15, attr.st.st_size);
}

TEST_F(FileInodeTest, getattrFromOverlay) {
  auto start = std::chrono::system_clock::now();
  // Allow ourselves up to 1 second of slop.
  //
  // When using files in the overlay we currently use the timestamp from the
  // underlying file system.  Some file systems only provide second-level
  // granularity.  Even on filesystems with higher granularity, the timestamp
  // is often stored based on a cached time value in the kernel that is only
  // updated on timer interrupts.  Therefore we might get a value slightly
  // older than the start time we computed.
  start -= std::chrono::seconds{1};

  mount_.addFile("dir/new_file.c", "hello\nworld\n");
  auto inode = mount_.getFileInode("dir/new_file.c");

  auto attr = getFileAttr(inode);
  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(12, attr.st.st_size);
  EXPECT_GE(attr.st.st_atim, start);
  EXPECT_GE(attr.st.st_mtim, start);
  EXPECT_GE(attr.st.st_ctim, start);
}

TEST_F(FileInodeTest, setattrTruncateAll) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_SIZE);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(0, attr.st.st_size);

  EXPECT_FILE_INODE(inode, "", 0644);
}

TEST_F(FileInodeTest, setattrTruncatePartial) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};
  desired.st_size = 4;
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_SIZE);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(4, attr.st.st_size);

  EXPECT_FILE_INODE(inode, "This", 0644);
}

TEST_F(FileInodeTest, setattrBiggerSize) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};
  desired.st_size = 30;
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_SIZE);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(30, attr.st.st_size);

  StringPiece expectedContents(
      "This is a.txt.\n"
      "\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
      30);
  EXPECT_FILE_INODE(inode, expectedContents, 0644);
}

TEST_F(FileInodeTest, setattrPermissions) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};

  for (int n = 0; n <= 0777; ++n) {
    desired.st_mode = n;
    auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_MODE);

    BASIC_ATTR_CHECKS(inode, attr);
    EXPECT_EQ((S_IFREG | n), attr.st.st_mode);
    EXPECT_EQ(15, attr.st.st_size);
    EXPECT_FILE_INODE(inode, "This is a.txt.\n", n);
  }
}

TEST_F(FileInodeTest, setattrFileType) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};

  // File type bits in the mode should be ignored.
  desired.st_mode = S_IFLNK | 0755;
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_MODE);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0755), attr.st.st_mode)
      << "File type bits in the mode should be ignored by setattr()";
  EXPECT_EQ(15, attr.st.st_size);
  EXPECT_FILE_INODE(inode, "This is a.txt.\n", 0755);
}

TEST_F(FileInodeTest, setattrUid) {
  auto inode = mount_.getFileInode("dir/a.txt");
  uid_t uid = inode->getMount()->getUid();
  struct stat desired = {};
  desired.st_uid = uid + 1;

  // We do not support changing the UID to something else.
  EXPECT_THROW_ERRNO(setFileAttr(inode, desired, FUSE_SET_ATTR_UID), EACCES);
  auto attr = BASIC_ATTR_CHECKS(inode);
  EXPECT_EQ(uid, attr.st.st_uid);

  // But setting the UID to the same value should succeed.
  desired.st_uid = uid;
  attr = setFileAttr(inode, desired, FUSE_SET_ATTR_UID);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(15, attr.st.st_size);
  EXPECT_EQ(uid, attr.st.st_uid);
}

TEST_F(FileInodeTest, setattrGid) {
  auto inode = mount_.getFileInode("dir/a.txt");
  gid_t gid = inode->getMount()->getGid();
  struct stat desired = {};
  desired.st_gid = gid + 1;

  // We do not support changing the GID to something else.
  EXPECT_THROW_ERRNO(setFileAttr(inode, desired, FUSE_SET_ATTR_GID), EACCES);
  auto attr = BASIC_ATTR_CHECKS(inode);
  EXPECT_EQ(gid, attr.st.st_gid);

  // But setting the GID to the same value should succeed.
  desired.st_gid = gid;
  attr = setFileAttr(inode, desired, FUSE_SET_ATTR_GID);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(15, attr.st.st_size);
  EXPECT_EQ(gid, attr.st.st_gid);
}

TEST_F(FileInodeTest, setattrAtime) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};

  // Set the atime to a specific value
  desired.st_atim.tv_sec = 1234;
  desired.st_atim.tv_nsec = 5678;
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_ATIME);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st.st_atime);
  EXPECT_EQ(1234, attr.st.st_atim.tv_sec);
  EXPECT_EQ(5678, attr.st.st_atim.tv_nsec);

  // Ask to set the atime to the current time
  auto start = std::chrono::system_clock::now();
  desired.st_atim.tv_sec = 8765;
  desired.st_atim.tv_nsec = 4321;
  attr = setFileAttr(inode, desired, FUSE_SET_ATTR_ATIME_NOW);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_GE(attr.st.st_atim, start);
}

TEST_F(FileInodeTest, setattrMtime) {
  auto inode = mount_.getFileInode("dir/a.txt");
  struct stat desired = {};

  // Set the mtime to a specific value
  desired.st_mtim.tv_sec = 1234;
  desired.st_mtim.tv_nsec = 5678;
  auto attr = setFileAttr(inode, desired, FUSE_SET_ATTR_MTIME);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st.st_mtime);
  EXPECT_EQ(1234, attr.st.st_mtim.tv_sec);
  EXPECT_EQ(5678, attr.st.st_mtim.tv_nsec);

  // Ask to set the mtime to the current time
  auto start = std::chrono::system_clock::now();
  desired.st_mtim.tv_sec = 8765;
  desired.st_mtim.tv_nsec = 4321;
  attr = setFileAttr(inode, desired, FUSE_SET_ATTR_MTIME_NOW);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_GE(attr.st.st_mtim, start);
}

// TODO: test multiple flags together
// TODO: ensure ctime is updated after every call to setattr()
// TODO: ensure mtime is updated after opening a file, writing to it, then
// closing it.
