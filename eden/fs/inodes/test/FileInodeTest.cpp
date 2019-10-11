/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/FileInode.h"

#include <folly/Format.h>
#include <folly/Range.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <chrono>

#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/utils/StatTimes.h"

using namespace facebook::eden;
using folly::StringPiece;
using folly::literals::string_piece_literals::operator""_sp;
using namespace std::chrono_literals;

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

/*
 * Helper functions for comparing timespec structs from file attributes
 * against C++11-style time_point objects.
 */
bool operator<(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) < tp;
}
bool operator<=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) <= tp;
}
bool operator>(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) > tp;
}
bool operator>=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) >= tp;
}
bool operator!=(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) != tp;
}
bool operator==(const timespec& ts, std::chrono::system_clock::time_point tp) {
  return folly::to<std::chrono::system_clock::time_point>(ts) == tp;
}

namespace {

Dispatcher::Attr getFileAttr(const FileInodePtr& inode) {
  auto attrFuture = inode->getattr();
  // We unfortunately can't use an ASSERT_* check here, since it tries
  // to return from the function normally, rather than throwing.
  if (!attrFuture.isReady()) {
    // Use ADD_FAILURE() so that any SCOPED_TRACE() data will be reported,
    // then throw an exception.
    ADD_FAILURE() << "getattr() future is not ready";
    throw std::runtime_error("getattr future is not ready");
  }
  return std::move(attrFuture).get();
}

Dispatcher::Attr setFileAttr(
    const FileInodePtr& inode,
    const fuse_setattr_in& desired) {
  auto attrFuture = inode->setattr(desired);
  if (!attrFuture.isReady()) {
    ADD_FAILURE() << "setattr() future is not ready";
    throw std::runtime_error("setattr future is not ready");
  }
  return std::move(attrFuture).get();
}

/**
 * Helper function used by BASIC_ATTR_CHECKS()
 */
void basicAttrChecks(const FileInodePtr& inode, const Dispatcher::Attr& attr) {
  EXPECT_EQ(inode->getNodeId().getRawValue(), attr.st.st_ino);
  EXPECT_EQ(1, attr.st.st_nlink);
  EXPECT_EQ(inode->getMount()->getOwner().uid, attr.st.st_uid);
  EXPECT_EQ(inode->getMount()->getOwner().gid, attr.st.st_gid);
  EXPECT_EQ(0, attr.st.st_rdev);
  EXPECT_GT(attr.st.st_atime, 0);
  EXPECT_GT(attr.st.st_mtime, 0);
  EXPECT_GT(attr.st.st_ctime, 0);
  EXPECT_GT(attr.st.st_blksize, 0);

  // Note that st_blocks always refers to 512B blocks, and is not related to
  // the block size reported in st_blksize.
  //
  // Eden doesn't really store data in blocks internally, and instead simply
  // computes the value in st_blocks based on st_size.  This is mainly so that
  // applications like "du" will report mostly sane results.
  if (attr.st.st_size == 0) {
    EXPECT_EQ(0, attr.st.st_blocks);
  } else {
    EXPECT_GE(512 * attr.st.st_blocks, attr.st.st_size);
    EXPECT_LT(512 * (attr.st.st_blocks - 1), attr.st.st_size);
  }
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
    // Default to a nonzero time.
    mount_.getClock().advance(9876min);

    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.setFiles({{"dir/a.txt", "This is a.txt.\n"},
                      {"dir/sub/b.txt", "This is b.txt.\n"}});
    mount_.initialize(builder);
  }

  TestMount mount_;
};

TEST_F(FileInodeTest, getType) {
  auto dir = mount_.getTreeInode("dir/sub");
  auto regularFile = mount_.getFileInode("dir/a.txt");
  EXPECT_EQ(dtype_t::Dir, dir->getType());
  EXPECT_EQ(dtype_t::Regular, regularFile->getType());
}

TEST_F(FileInodeTest, getattrFromBlob) {
  auto inode = mount_.getFileInode("dir/a.txt");
  auto attr = getFileAttr(inode);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(15, attr.st.st_size);
  EXPECT_EQ(1, attr.st.st_blocks);
}

TEST_F(FileInodeTest, getattrFromOverlay) {
  auto start = mount_.getClock().getTimePoint();

  mount_.addFile("dir/new_file.c", "hello\nworld\n");
  auto inode = mount_.getFileInode("dir/new_file.c");

  auto attr = getFileAttr(inode);
  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(12, attr.st.st_size);
  EXPECT_EQ(1, attr.st.st_blocks);
  EXPECT_EQ(stAtimepoint(attr.st), start);
  EXPECT_EQ(stMtimepoint(attr.st), start);
  EXPECT_EQ(stCtimepoint(attr.st), start);
}

void testSetattrTruncateAll(TestMount& mount) {
  auto inode = mount.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};
  desired.valid = FATTR_SIZE;
  auto attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(0, attr.st.st_size);
  EXPECT_EQ(0, attr.st.st_blocks);

  EXPECT_FILE_INODE(inode, "", 0644);
}

TEST_F(FileInodeTest, setattrTruncateAll) {
  testSetattrTruncateAll(mount_);
}

TEST_F(FileInodeTest, setattrTruncateAllMaterialized) {
  // Modify the inode before running the test, so that
  // it will be materialized in the overlay.
  auto inode = mount_.getFileInode("dir/a.txt");
  inode->write("THIS IS A.TXT.\n", 0);
  inode.reset();

  testSetattrTruncateAll(mount_);
}

TEST_F(FileInodeTest, setattrTruncatePartial) {
  auto inode = mount_.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};
  desired.size = 4;
  desired.valid = FATTR_SIZE;
  auto attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st.st_mode);
  EXPECT_EQ(4, attr.st.st_size);

  EXPECT_FILE_INODE(inode, "This", 0644);
}

TEST_F(FileInodeTest, setattrBiggerSize) {
  auto inode = mount_.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};
  desired.size = 30;
  desired.valid = FATTR_SIZE;
  auto attr = setFileAttr(inode, desired);

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
  fuse_setattr_in desired = {};

  for (int n = 0; n <= 0777; ++n) {
    desired.mode = n;
    desired.valid = FATTR_MODE;
    auto attr = setFileAttr(inode, desired);

    BASIC_ATTR_CHECKS(inode, attr);
    EXPECT_EQ((S_IFREG | n), attr.st.st_mode);
    EXPECT_EQ(15, attr.st.st_size);
    EXPECT_FILE_INODE(inode, "This is a.txt.\n", n);
  }
}

TEST_F(FileInodeTest, setattrFileType) {
  auto inode = mount_.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};

  // File type bits in the mode should be ignored.
  desired.mode = S_IFLNK | 0755;
  desired.valid = FATTR_MODE;
  auto attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0755), attr.st.st_mode)
      << "File type bits in the mode should be ignored by setattr()";
  EXPECT_EQ(15, attr.st.st_size);
  EXPECT_FILE_INODE(inode, "This is a.txt.\n", 0755);
}

TEST_F(FileInodeTest, setattrAtime) {
  auto inode = mount_.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};
  desired.valid = FATTR_ATIME;

  // Set the atime to a specific value
  desired.atime = 1234;
  desired.atimensec = 5678;
  auto attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st.st_atime);
  EXPECT_EQ(1234, stAtime(attr.st).tv_sec);
  EXPECT_EQ(5678, stAtime(attr.st).tv_nsec);

  mount_.getClock().advance(10min);

  // Ask to set the atime to the current time
  desired.atime = 8765;
  desired.atimensec = 4321;
  desired.valid = FATTR_ATIME_NOW;
  attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(
      mount_.getClock().getTimePoint(),
      folly::to<FakeClock::time_point>(stAtime(attr.st)));
}

void testSetattrMtime(TestMount& mount) {
  auto inode = mount.getFileInode("dir/a.txt");
  fuse_setattr_in desired = {};

  // Set the mtime to a specific value
  desired.mtime = 1234;
  desired.mtimensec = 5678;
  desired.valid = FATTR_MTIME;
  auto attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st.st_mtime);
  EXPECT_EQ(1234, stMtime(attr.st).tv_sec);
  EXPECT_EQ(5678, stMtime(attr.st).tv_nsec);

  // Ask to set the mtime to the current time
  mount.getClock().advance(1234min);
  auto start = mount.getClock().getTimePoint();
  desired.mtime = 8765;
  desired.mtimensec = 4321;
  desired.valid = FATTR_MTIME_NOW;
  attr = setFileAttr(inode, desired);

  BASIC_ATTR_CHECKS(inode, attr);
  EXPECT_EQ(start, folly::to<FakeClock::time_point>(stMtime(attr.st)));
}

TEST_F(FileInodeTest, setattrMtime) {
  testSetattrMtime(mount_);
}

TEST_F(FileInodeTest, setattrMtimeMaterialized) {
  // Modify the inode before running the test, so that
  // it will be materialized in the overlay.
  auto inode = mount_.getFileInode("dir/a.txt");
  inode->write("THIS IS A.TXT.\n", 0);
  inode.reset();

  testSetattrMtime(mount_);
}

namespace {
bool isInodeMaterialized(const TreeInodePtr& inode) {
  return inode->getContents().wlock()->isMaterialized();
}
} // namespace

TEST_F(FileInodeTest, writingMaterializesParent) {
  auto inode = mount_.getFileInode("dir/sub/b.txt");
  auto parent = mount_.getTreeInode("dir/sub");
  auto grandparent = mount_.getTreeInode("dir");

  EXPECT_EQ(false, isInodeMaterialized(grandparent));
  EXPECT_EQ(false, isInodeMaterialized(parent));

  auto written = inode->write("abcd", 0).get();
  EXPECT_EQ(4, written);

  EXPECT_EQ(true, isInodeMaterialized(grandparent));
  EXPECT_EQ(true, isInodeMaterialized(parent));
}

TEST_F(FileInodeTest, truncatingMaterializesParent) {
  auto inode = mount_.getFileInode("dir/sub/b.txt");
  auto parent = mount_.getTreeInode("dir/sub");
  auto grandparent = mount_.getTreeInode("dir");

  EXPECT_EQ(false, isInodeMaterialized(grandparent));
  EXPECT_EQ(false, isInodeMaterialized(parent));

  fuse_setattr_in attr;
  attr.valid = FATTR_SIZE;
  attr.size = 0;
  (void)inode->setattr(attr).get(0ms);

  EXPECT_EQ(true, isInodeMaterialized(grandparent));
  EXPECT_EQ(true, isInodeMaterialized(parent));
}

TEST(FileInode, truncatingDuringLoad) {
  FakeTreeBuilder builder;
  builder.setFiles({{"notready.txt", "Contents not ready.\n"}});

  TestMount mount_;
  mount_.initialize(builder, false);

  auto inode = mount_.getFileInode("notready.txt");

  auto backingStore = mount_.getBackingStore();
  auto storedBlob = backingStore->getStoredBlob(*inode->getBlobHash());

  auto readAllFuture = inode->readAll();
  EXPECT_EQ(false, readAllFuture.isReady());

  {
    // Synchronously truncate the file while the load is in progress.
    fuse_setattr_in attr;
    attr.valid = FATTR_SIZE;
    attr.size = 0;
    (void)inode->setattr(attr).get(0ms);
    // Deallocate the handle here, closing the open file.
  }

  // Verify, from the caller's perspective, the load is complete (but empty).
  EXPECT_EQ(true, readAllFuture.isReady());
  EXPECT_EQ("", readAllFuture.value());

  // Now finish the ObjectStore load request to make sure the FileInode
  // handles the state correctly.
  storedBlob->setReady();
}

TEST(FileInode, readDuringLoad) {
  // Build a tree to test against, but do not mark the state ready yet
  FakeTreeBuilder builder;
  auto contents = "Contents not ready.\n"_sp;
  builder.setFiles({{"notready.txt", contents}});
  TestMount mount_;
  mount_.initialize(builder, false);

  // Load the inode and start reading the contents
  auto inode = mount_.getFileInode("notready.txt");
  auto dataFuture = inode->read(4096, 0);

  EXPECT_FALSE(dataFuture.isReady());

  // Make the backing store data ready now.
  builder.setAllReady();

  // The read() operation should have completed now.
  ASSERT_TRUE(dataFuture.isReady());
  EXPECT_EQ(contents, std::move(dataFuture).get().copyData());
}

TEST(FileInode, writeDuringLoad) {
  // Build a tree to test against, but do not mark the state ready yet
  FakeTreeBuilder builder;
  builder.setFiles({{"notready.txt", "Contents not ready.\n"}});
  TestMount mount_;
  mount_.initialize(builder, false);

  // Load the inode and start reading the contents
  auto inode = mount_.getFileInode("notready.txt");

  auto newContents = "TENTS"_sp;
  auto writeFuture = inode->write(newContents, 3);
  EXPECT_FALSE(writeFuture.isReady());

  // Make the backing store data ready now.
  builder.setAllReady();

  // The write() operation should have completed now.
  ASSERT_TRUE(writeFuture.isReady());
  EXPECT_EQ(newContents.size(), std::move(writeFuture).get());

  // We should be able to read back our modified data now.
  EXPECT_FILE_INODE(inode, "ConTENTS not ready.\n", 0644);
}

TEST(FileInode, truncateDuringLoad) {
  // Build a tree to test against, but do not mark the state ready yet
  FakeTreeBuilder builder;
  builder.setFiles({{"notready.txt", "Contents not ready.\n"}});
  TestMount mount_;
  mount_.initialize(builder, false);

  auto inode = mount_.getFileInode("notready.txt");

  // Start reading the contents
  auto dataFuture = inode->read(4096, 0);
  EXPECT_FALSE(dataFuture.isReady());

  // Truncate the file while the initial read is in progress. This should
  // immediately truncate the file even without needing to wait for the data
  // from the object store.
  fuse_setattr_in attr;
  attr.valid = FATTR_SIZE;
  attr.size = 0;
  (void)inode->setattr(attr).get(0ms);

  // The read should complete now too.
  ASSERT_TRUE(dataFuture.isReady());
  EXPECT_EQ("", std::move(dataFuture).get().copyData());

  // For good measure, test reading and writing some more.
  inode->write("foobar\n"_sp, 5).get(0ms);

  dataFuture = inode->read(4096, 0);
  ASSERT_TRUE(dataFuture.isReady());
  EXPECT_EQ("\0\0\0\0\0foobar\n"_sp, std::move(dataFuture).get().copyData());

  EXPECT_FILE_INODE(inode, "\0\0\0\0\0foobar\n"_sp, 0644);
}

TEST(FileInode, dropsCacheWhenFullyRead) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto hash = inode->getBlobHash().value();

  EXPECT_FALSE(blobCache->get(hash).blob);

  inode->read(4, 0).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  inode->read(4, 4).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  inode->read(4, 8).get(0ms);
  EXPECT_FALSE(blobCache->contains(hash));
}

TEST(FileInode, keepsCacheIfPartiallyReread) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto hash = inode->getBlobHash().value();

  EXPECT_FALSE(blobCache->contains(hash));

  inode->read(6, 0).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  inode->read(6, 6).get(0ms);
  EXPECT_FALSE(blobCache->contains(hash));

  inode->read(6, 0).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  // Evicts again on the second full read!
  inode->read(6, 6).get(0ms);
  EXPECT_FALSE(blobCache->contains(hash));
}

TEST(FileInode, dropsCacheWhenMaterialized) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto hash = inode->getBlobHash().value();

  EXPECT_FALSE(blobCache->get(hash).blob);

  inode->read(4, 0).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  inode->write("data"_sp, 0).get(0ms);
  EXPECT_FALSE(blobCache->contains(hash));
}

TEST(FileInode, dropsCacheWhenUnloaded) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto hash = inode->getBlobHash().value();

  inode->read(4, 0).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash));

  inode.reset();
  mount.getEdenMount()->getRootInode()->unloadChildrenNow();
  EXPECT_FALSE(blobCache->contains(hash));
}

TEST(FileInode, reloadsBlobIfCacheIsEvicted) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto hash = inode->getBlobHash().value();

  inode->read(4, 0).get(0ms);
  blobCache->clear();
  EXPECT_FALSE(blobCache->contains(hash));

  inode->read(4, 4).get(0ms);
  EXPECT_TRUE(blobCache->contains(hash))
      << "reading should insert hash " << hash << " into cache";
}

// TODO: test multiple flags together
// TODO: ensure ctime is updated after every call to setattr()
// TODO: ensure mtime is updated after opening a file, writing to it, then
// closing it.
