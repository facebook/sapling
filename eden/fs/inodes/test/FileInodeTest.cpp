/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/FileInode.h"

#include <fmt/format.h>
#include <folly/Range.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <chrono>

#include "eden/common/utils/StatTimes.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::StringPiece;
using folly::literals::string_piece_literals::operator""_sp;
using namespace std::chrono_literals;

constexpr auto materializationTimeoutLimit = 1000ms;

template <>
struct fmt::formatter<timespec> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(const timespec& ts, FormatContext& ctx) {
    return fmt::format_to(ctx.out(), "{}.{:09d}", ts.tv_sec, ts.tv_nsec);
  }
};

template <>
struct fmt::formatter<std::chrono::system_clock::time_point> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(
      const std::chrono::system_clock::time_point& tp,
      FormatContext& ctx) {
    auto duration = tp.time_since_epoch();
    auto secs = std::chrono::duration_cast<std::chrono::seconds>(duration);
    auto nsecs =
        std::chrono::duration_cast<std::chrono::nanoseconds>(duration - secs);
    return fmt::format_to(ctx.out(), "{}.{:09d}", secs.count(), nsecs.count());
  }
};

// Helper function to avoid GoogleTest formatting issues with chrono types
template <typename T>
std::string formatTimePoint(const T& tp) {
  return fmt::format("{}", tp);
}

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

struct stat getFileAttr(TestMount& mount, const FileInodePtr& inode) {
  auto attrFuture = inode->stat(ObjectFetchContext::getNullContext())
                        .semi()
                        .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  // We unfortunately can't use an ASSERT_* check here, since it tries
  // to return from the function normally, rather than throwing.
  if (!attrFuture.isReady()) {
    // Use ADD_FAILURE() so that any SCOPED_TRACE() data will be reported,
    // then throw an exception.
    ADD_FAILURE() << "getattr() future is not ready";
    throw std::runtime_error("getattr future is not ready");
  }

  return std::move(attrFuture).get(0ms);
}

struct stat setFileAttr(
    TestMount& mount,
    const FileInodePtr& inode,
    const DesiredMetadata& desired) {
  auto attrFuture =
      inode->setattr(desired, ObjectFetchContext::getNullContext())
          .semi()
          .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  if (!attrFuture.isReady()) {
    ADD_FAILURE() << "setattr() future is not ready";
    throw std::runtime_error("setattr future is not ready");
  }
  return std::move(attrFuture).get(0ms);
}

/**
 * Helper function used by BASIC_ATTR_XCHECKS()
 */
void basicAttrChecks(const FileInodePtr& inode, const struct stat& attr) {
  EXPECT_EQ(inode->getNodeId().getRawValue(), attr.st_ino);
  EXPECT_EQ(1, attr.st_nlink);
  EXPECT_EQ(inode->getMount()->getOwner().uid, attr.st_uid);
  EXPECT_EQ(inode->getMount()->getOwner().gid, attr.st_gid);
  EXPECT_EQ(0, attr.st_rdev);
  EXPECT_GT(attr.st_atime, 0);
  EXPECT_GT(attr.st_mtime, 0);
  EXPECT_GT(attr.st_ctime, 0);
  EXPECT_GT(attr.st_blksize, 0);

  // Note that st_blocks always refers to 512B blocks, and is not related to
  // the block size reported in st_blksize.
  //
  // Eden doesn't really store data in blocks internally, and instead simply
  // computes the value in st_blocks based on st_size.  This is mainly so that
  // applications like "du" will report mostly sane results.
  if (attr.st_size == 0) {
    EXPECT_EQ(0, attr.st_blocks);
  } else {
    EXPECT_GE(512 * attr.st_blocks, attr.st_size);
    EXPECT_LT(512 * (attr.st_blocks - 1), attr.st_size);
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
#define BASIC_ATTR_XCHECKS(inode, ...)                                        \
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
    builder.setFiles(
        {{"dir/a.txt", "This is a.txt.\n"},
         {"dir/sub/b.txt", "This is b.txt.\n"}});
    mount_.initialize(builder);
  }

  /**
   * Queue used in addNewMaterializationsToInodeTraceBus test to store inode
   * materializations. Created as an attribute here so that it is destructed
   * after mount_ is destructed to ensure any materializations in the mount will
   * have an active queue to be entered into
   */
  folly::UnboundedQueue<
      InodeTraceEvent,
      /*SingleProducer=*/true,
      /*SingleConsumer=*/true,
      /*MayBlock=*/false>
      queue_;
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
  auto attr = getFileAttr(mount_, inode);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st_mode);
  EXPECT_EQ(15, attr.st_size);
  EXPECT_EQ(1, attr.st_blocks);
}

TEST_F(FileInodeTest, getattrFromOverlay) {
  auto start = mount_.getClock().getTimePoint();

  mount_.addFile("dir/new_file.c", "hello\nworld\n");
  auto inode = mount_.getFileInode("dir/new_file.c");

  auto attr = getFileAttr(mount_, inode);
  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st_mode);
  EXPECT_EQ(12, attr.st_size);
  EXPECT_EQ(1, attr.st_blocks);
  EXPECT_EQ(formatTimePoint(stAtimepoint(attr)), formatTimePoint(start));
  EXPECT_EQ(formatTimePoint(stMtimepoint(attr)), formatTimePoint(start));
  EXPECT_EQ(formatTimePoint(stCtimepoint(attr)), formatTimePoint(start));
}

void testSetattrTruncateAll(TestMount& mount) {
  auto inode = mount.getFileInode("dir/a.txt");
  DesiredMetadata desired;
  desired.size = 0;
  auto attr = setFileAttr(mount, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st_mode);
  EXPECT_EQ(0, attr.st_size);
  EXPECT_EQ(0, attr.st_blocks);

  EXPECT_FILE_INODE(inode, "", 0644);
}

TEST_F(FileInodeTest, setattrTruncateAll) {
  testSetattrTruncateAll(mount_);
}

TEST_F(FileInodeTest, setattrTruncateAllMaterialized) {
  // Modify the inode before running the test, so that
  // it will be materialized in the overlay.
  auto inode = mount_.getFileInode("dir/a.txt");
  auto written =
      inode->write("THIS IS A.TXT.\n", 0, ObjectFetchContext::getNullContext())
          .get();
  EXPECT_EQ(15, written);
  EXPECT_TRUE(inode->isMaterialized());
  inode.reset();

  testSetattrTruncateAll(mount_);
}

TEST_F(FileInodeTest, setattrTruncatePartial) {
  auto inode = mount_.getFileInode("dir/a.txt");
  DesiredMetadata desired;
  desired.size = 4;
  auto attr = setFileAttr(mount_, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st_mode);
  EXPECT_EQ(4, attr.st_size);

  EXPECT_FILE_INODE(inode, "This", 0644);
}

TEST_F(FileInodeTest, setattrBiggerSize) {
  auto inode = mount_.getFileInode("dir/a.txt");
  DesiredMetadata desired;
  desired.size = 30;
  auto attr = setFileAttr(mount_, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0644), attr.st_mode);
  EXPECT_EQ(30, attr.st_size);

  StringPiece expectedContents(
      "This is a.txt.\n"
      "\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
      30);
  EXPECT_FILE_INODE(inode, expectedContents, 0644);
}

TEST_F(FileInodeTest, setattrPermissions) {
  auto inode = mount_.getFileInode("dir/a.txt");
  DesiredMetadata desired;

  for (int n = 0; n <= 0777; ++n) {
    desired.mode = n;
    auto attr = setFileAttr(mount_, inode, desired);

    BASIC_ATTR_XCHECKS(inode, attr);
    EXPECT_EQ((S_IFREG | n), attr.st_mode);
    EXPECT_EQ(15, attr.st_size);
    EXPECT_FILE_INODE(inode, "This is a.txt.\n", n);
  }
}

TEST_F(FileInodeTest, setattrFileType) {
  auto inode = mount_.getFileInode("dir/a.txt");
  DesiredMetadata desired;

  // File type bits in the mode should be ignored.
  desired.mode = S_IFLNK | 0755;
  auto attr = setFileAttr(mount_, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ((S_IFREG | 0755), attr.st_mode)
      << "File type bits in the mode should be ignored by setattr()";
  EXPECT_EQ(15, attr.st_size);
  EXPECT_FILE_INODE(inode, "This is a.txt.\n", 0755);
}

TEST_F(FileInodeTest, setattrAtime) {
  auto inode = mount_.getFileInode("dir/a.txt");
  DesiredMetadata desired;

  // Set the atime to a specific value
  timespec atime;
  atime.tv_sec = 1234;
  atime.tv_nsec = 5678;
  desired.atime = atime;

  auto attr = setFileAttr(mount_, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st_atime);
  EXPECT_EQ(1234, stAtime(attr).tv_sec);
  EXPECT_EQ(5678, stAtime(attr).tv_nsec);

  mount_.getClock().advance(10min);

  // Ask to set the atime to the current time
  desired.atime = mount_.getClock().getRealtime();

  attr = setFileAttr(mount_, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ(
      formatTimePoint(mount_.getClock().getTimePoint()),
      formatTimePoint(folly::to<FakeClock::time_point>(stAtime(attr))));
}

void testSetattrMtime(TestMount& mount) {
  auto inode = mount.getFileInode("dir/a.txt");
  DesiredMetadata desired;

  // Set the mtime to a specific value
  timespec mtime;
  mtime.tv_sec = 1234;
  mtime.tv_nsec = 5678;
  desired.mtime = mtime;

  auto attr = setFileAttr(mount, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ(1234, attr.st_mtime);
  EXPECT_EQ(1234, stMtime(attr).tv_sec);
  EXPECT_EQ(5678, stMtime(attr).tv_nsec);

  // Ask to set the mtime to the current time
  mount.getClock().advance(1234min);
  auto start = mount.getClock().getTimePoint();
  desired.mtime = mount.getClock().getRealtime();

  attr = setFileAttr(mount, inode, desired);

  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ(
      formatTimePoint(start),
      formatTimePoint(folly::to<FakeClock::time_point>(stMtime(attr))));
}

TEST_F(FileInodeTest, setattrMtime) {
  testSetattrMtime(mount_);
}

TEST_F(FileInodeTest, setattrMtimeMaterialized) {
  // Modify the inode before running the test, so that
  // it will be materialized in the overlay.
  auto inode = mount_.getFileInode("dir/a.txt");
  auto written =
      inode->write("THIS IS A.TXT.\n", 0, ObjectFetchContext::getNullContext())
          .get();
  EXPECT_EQ(15, written);
  EXPECT_TRUE(inode->isMaterialized());
  inode.reset();

  testSetattrMtime(mount_);
}

TEST_F(FileInodeTest, writingMaterializesParent) {
  auto inode = mount_.getFileInode("dir/sub/b.txt");
  auto parent = mount_.getTreeInode("dir/sub");
  auto grandparent = mount_.getTreeInode("dir");

  EXPECT_EQ(false, grandparent->isMaterialized());
  EXPECT_EQ(false, parent->isMaterialized());

  auto written =
      inode->write("abcd", 0, ObjectFetchContext::getNullContext()).get();
  EXPECT_EQ(4, written);

  EXPECT_EQ(true, grandparent->isMaterialized());
  EXPECT_EQ(true, parent->isMaterialized());
}

TEST_F(FileInodeTest, truncatingMaterializesParent) {
  auto inode = mount_.getFileInode("dir/sub/b.txt");
  auto parent = mount_.getTreeInode("dir/sub");
  auto grandparent = mount_.getTreeInode("dir");

  EXPECT_EQ(false, grandparent->isMaterialized());
  EXPECT_EQ(false, parent->isMaterialized());

  DesiredMetadata desired;
  desired.size = 0;
  (void)inode->setattr(desired, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(true, grandparent->isMaterialized());
  EXPECT_EQ(true, parent->isMaterialized());
}

TEST_F(FileInodeTest, addNewMaterializationsToInodeTraceBus) {
  auto& trace_bus = mount_.getEdenMount()->getInodeTraceBus();

  auto inode_a = mount_.getFileInode("dir/a.txt");
  auto inode_b = mount_.getFileInode("dir/sub/b.txt");
  auto inode_sub = mount_.getTreeInode("dir/sub");
  auto inode_dir = mount_.getTreeInode("dir");

  // Detect inode materialization events and add events to synchronized queue
  auto handle = trace_bus.subscribeFunction(
      fmt::format(
          "fileInodeTest-{}", mount_.getEdenMount()->getPath().basename()),
      [&](const InodeTraceEvent& event) {
        if (event.eventType == InodeEventType::MATERIALIZE) {
          queue_.enqueue(event);
        }
      });

  // Wait for any initial materialization events to complete
  while (queue_.try_dequeue_for(materializationTimeoutLimit).has_value()) {
  };

  // Test writing a file
  inode_a->write("abcd", 0, ObjectFetchContext::getNullContext()).get();
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::START, inode_a->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::START, inode_dir->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::END, inode_dir->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::END, inode_a->getNodeId()));

  // Test truncating a file
  DesiredMetadata desired;
  desired.size = 0;
  (void)inode_b->setattr(desired, ObjectFetchContext::getNullContext())
      .get(0ms);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::START, inode_b->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::START, inode_sub->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::END, inode_sub->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue_, InodeEventProgress::END, inode_b->getNodeId()));

  // Ensure we do not count any other materializations a second time
  EXPECT_FALSE(queue_.try_dequeue_for(materializationTimeoutLimit).has_value());
}

#ifdef __linux__
TEST_F(FileInodeTest, fallocate) {
  mount_.addFile("dir/fallocate_file", "");
  auto inode = mount_.getFileInode("dir/fallocate_file");
  inode->fallocate(0, 42, ObjectFetchContext::getNullContext()).get(0ms);

  auto attr = getFileAttr(mount_, inode);
  BASIC_ATTR_XCHECKS(inode, attr);
  EXPECT_EQ(42, attr.st_size);
}
#endif

TEST(FileInode, truncatingDuringLoad) {
  FakeTreeBuilder builder;
  builder.setFiles({{"notready.txt", "Contents not ready.\n"}});

  TestMount mount_;
  mount_.initialize(builder, false);

  auto inode = mount_.getFileInode("notready.txt");

  auto backingStore = mount_.getBackingStore();
  auto storedBlob = backingStore->getStoredBlob(*inode->getObjectId());

  auto readAllFuture = inode->readAll(ObjectFetchContext::getNullContext());
  EXPECT_EQ(false, readAllFuture.isReady());

  {
    // Synchronously truncate the file while the load is in progress.
    DesiredMetadata desired;
    desired.size = 0;
    (void)inode->setattr(desired, ObjectFetchContext::getNullContext())
        .get(0ms);
    // Deallocate the handle here, closing the open file.
  }

  // Verify, from the caller's perspective, the load is complete (but empty).
  EXPECT_EQ("", std::move(readAllFuture).get(0ms));

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
  auto dataFuture = inode->read(4096, 0, ObjectFetchContext::getNullContext())
                        .thenValue([](std::tuple<BufVec, bool> readRes) {
                          auto [data, isEof] = std::move(readRes);
                          EXPECT_EQ(true, isEof);
                          return data->moveToFbString();
                        });

  EXPECT_FALSE(dataFuture.isReady());

  // Make the backing store data ready now.
  builder.setAllReady();

  // The read() operation should have completed now.
  EXPECT_EQ(contents, std::move(dataFuture).get(0ms));
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
  auto writeFuture =
      inode->write(newContents, 3, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(writeFuture.isReady());

  // Make the backing store data ready now.
  builder.setAllReady();

  // The write() operation should have completed now.
  EXPECT_EQ(newContents.size(), std::move(writeFuture).get(0ms));

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
  auto dataFuture = inode->read(4096, 0, ObjectFetchContext::getNullContext())
                        .thenValue([](std::tuple<BufVec, bool> readRes) {
                          auto [data, isEof] = std::move(readRes);
                          EXPECT_EQ(true, isEof);
                          return data->moveToFbString();
                        })
                        .semi()
                        .via(mount_.getServerExecutor().get());
  mount_.drainServerExecutor();
  EXPECT_FALSE(dataFuture.isReady());

  // Truncate the file while the initial read is in progress. This should
  // immediately truncate the file even without needing to wait for the data
  // from the object store.
  DesiredMetadata desired;
  desired.size = 0;
  (void)inode->setattr(desired, ObjectFetchContext::getNullContext()).get(0ms);

  // The read should complete now too.
  mount_.drainServerExecutor();
  EXPECT_EQ("", std::move(dataFuture).get(0ms));

  // For good measure, test reading and writing some more.
  inode->write("foobar\n"_sp, 5, ObjectFetchContext::getNullContext()).get(0ms);

  dataFuture = inode->read(4096, 0, ObjectFetchContext::getNullContext())
                   .thenValue([](std::tuple<BufVec, bool> readRes) {
                     auto [data, isEof] = std::move(readRes);
                     EXPECT_EQ(false, isEof);
                     return data->moveToFbString();
                   })
                   .semi()
                   .via(mount_.getServerExecutor().get());
  mount_.drainServerExecutor();
  ASSERT_TRUE(dataFuture.isReady());
  EXPECT_EQ("\0\0\0\0\0foobar\n"_sp, std::move(dataFuture).get(0ms));

  EXPECT_FILE_INODE(inode, "\0\0\0\0\0foobar\n"_sp, 0644);
}

TEST(FileInode, dropsCacheWhenFullyRead) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto id = inode->getObjectId().value();

  EXPECT_FALSE(blobCache->get(id).object);

  inode->read(4, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  inode->read(4, 4, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  inode->read(4, 8, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_FALSE(blobCache->contains(id));
}

TEST(FileInode, keepsCacheIfPartiallyReread) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto id = inode->getObjectId().value();

  EXPECT_FALSE(blobCache->contains(id));

  inode->read(6, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  inode->read(6, 6, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_FALSE(blobCache->contains(id));

  inode->read(6, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  // Evicts again on the second full read!
  inode->read(6, 6, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_FALSE(blobCache->contains(id));
}

TEST(FileInode, dropsCacheWhenMaterialized) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto id = inode->getObjectId().value();

  EXPECT_FALSE(blobCache->get(id).object);

  inode->read(4, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  inode->write("data"_sp, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(inode->isMaterialized());
  EXPECT_FALSE(blobCache->contains(id));
}

TEST(FileInode, dropsCacheWhenUnloaded) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto id = inode->getObjectId().value();

  inode->read(4, 0, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id));

  inode.reset();
  mount.getEdenMount()->getRootInode()->unloadChildrenNow();
  EXPECT_FALSE(blobCache->contains(id));
}

TEST(FileInode, reloadsBlobIfCacheIsEvicted) {
  FakeTreeBuilder builder;
  builder.setFiles({{"bigfile.txt", "1234567890ab"}});
  TestMount mount{builder};
  auto blobCache = mount.getBlobCache();

  auto inode = mount.getFileInode("bigfile.txt");
  auto id = inode->getObjectId().value();

  inode->read(4, 0, ObjectFetchContext::getNullContext()).get(0ms);
  blobCache->clear();
  EXPECT_FALSE(blobCache->contains(id));

  inode->read(4, 4, ObjectFetchContext::getNullContext()).get(0ms);
  EXPECT_TRUE(blobCache->contains(id))
      << fmt::format("reading should insert id {} into cache", id);
}

// TODO: test multiple flags together
// TODO: ensure ctime is updated after every call to setattr()
// TODO: ensure mtime is updated after opening a file, writing to it, then
// closing it.

#endif
