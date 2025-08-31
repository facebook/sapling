/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <memory>

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/inodes/sqlitecatalog/SqliteInodeCatalog.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::ByteRange;
using folly::StringPiece;
using std::make_shared;
using std::string;
using ::testing::HasSubstr;
using ::testing::UnorderedElementsAre;

namespace {

class TestDir;

class TestOverlay : public std::enable_shared_from_this<TestOverlay> {
 public:
  explicit TestOverlay(InodeCatalogType type);

  /*
   * Initialize the TestOverlay object.
   *
   * Returns the root directory.
   */
  TestDir init();

  const AbsolutePath& overlayPath() const {
    return fcs_.getLocalDir();
  }

  FsFileContentStore& fcs() {
    return fcs_;
  }

  InodeCatalog* inodeCatalog() {
    return inodeCatalog_.get();
  }

  InodeNumber getNextInodeNumber() {
    return InodeNumber(nextInodeNumber_);
  }

  InodeNumber allocateInodeNumber() {
    InodeNumber result(nextInodeNumber_);
    ++nextInodeNumber_;
    return result;
  }

  void closeCleanly() {
    inodeCatalog_->close(getNextInodeNumber());
    if (type_ != InodeCatalogType::Legacy) {
      fcs_.close();
    }
  }

  void corruptInodeHeader(InodeNumber number, StringPiece headerData) {
    XCHECK_EQ(headerData.size(), FsFileContentStore::kHeaderLength);
    auto overlayFile = fcs_.openFileNoVerify(number);
    auto ret = folly::pwriteFull(
        std::get<folly::File>(overlayFile).fd(),
        headerData.data(),
        headerData.size(),
        0);
    folly::checkUnixError(ret, "failed to replace file inode header");
  }

  void recreateSqliteInodeCatalog();

  std::shared_ptr<EdenConfig> getTestConfig() {
    return testConfig_;
  }

 private:
  folly::test::TemporaryDirectory tmpDir_;
  AbsolutePath tmpDirPath_;
  FsFileContentStore fcs_;
  std::unique_ptr<InodeCatalog> inodeCatalog_;
  InodeCatalogType type_;
  uint64_t nextInodeNumber_{0};
  std::shared_ptr<EdenConfig> testConfig_;
};

class TestFile {
 public:
  TestFile(
      std::shared_ptr<TestOverlay> overlay,
      InodeNumber number,
      folly::File file)
      : overlay_(std::move(overlay)), number_(number), file_(std::move(file)) {}

  InodeNumber number() const {
    return number_;
  }

 private:
  std::shared_ptr<TestOverlay> overlay_;
  InodeNumber number_;
  folly::File file_;
};

class TestDir {
 public:
  TestDir(std::shared_ptr<TestOverlay> overlay, InodeNumber number)
      : overlay_(std::move(overlay)), number_(number) {}

  InodeNumber number() const {
    return number_;
  }

  TestDir mkdir(
      StringPiece name,
      std::optional<ObjectId> id = std::nullopt,
      mode_t permissions = 0755) {
    auto number = addEntry(name, id, S_IFDIR | (permissions & 07777));
    return TestDir(overlay_, number);
  }

  TestDir linkFile(
      InodeNumber number,
      StringPiece name,
      std::optional<ObjectId> id = std::nullopt,
      mode_t permissions = 0755) {
    addEntry(name, id, S_IFREG | (permissions & 07777), number.get());
    return TestDir(overlay_, number);
  }

  TestFile create(
      StringPiece name,
      ByteRange contents,
      std::optional<ObjectId> id = std::nullopt,
      mode_t permissions = 0644) {
    auto number = addEntry(name, id, S_IFREG | (permissions & 07777));
    // The file should only be created in the overlay if it is materialized
    folly::File file;
    if (!id.has_value()) {
      file = std::get<folly::File>(
          overlay_->fcs().createOverlayFile(number, contents));
    }
    return TestFile(overlay_, number, std::move(file));
  }

  TestFile create(
      StringPiece name,
      StringPiece contents,
      std::optional<ObjectId> id = std::nullopt,
      mode_t permissions = 0644) {
    return create(name, ByteRange(contents), id, permissions);
  }

  void save() {
    overlay_->inodeCatalog()->saveOverlayDir(
        number_, overlay::OverlayDir{contents_});
  }

 private:
  InodeNumber addEntry(
      StringPiece name,
      std::optional<ObjectId> id,
      mode_t mode,
      uint64_t number = 0) {
    auto insertResult =
        contents_.entries()->emplace(name, overlay::OverlayEntry{});
    if (!insertResult.second) {
      throw std::runtime_error(
          fmt::format("an entry named \"{}\" already exists", name));
    }

    if (number == 0) {
      number = overlay_->allocateInodeNumber().get();
    }
    auto& entry = insertResult.first->second;
    entry.mode() = mode;
    entry.inodeNumber() = static_cast<int64_t>(number);
    if (id) {
      auto idBytes = id->getBytes();
      entry.hash() = std::string{

          reinterpret_cast<const char*>(idBytes.data()), idBytes.size()};
    }
    return InodeNumber(number);
  }

  std::shared_ptr<TestOverlay> overlay_;
  InodeNumber number_;
  overlay::OverlayDir contents_;
};

TestOverlay::TestOverlay(InodeCatalogType type)
    : tmpDir_(makeTempDir()),
      tmpDirPath_(canonicalPath(tmpDir_.path().string())),
      // fsck will write its output in a sibling directory to the overlay,
      // so make sure we put the overlay at least 1 directory deep inside our
      // temporary directory
      fcs_(tmpDirPath_ + "overlay"_pc),
      type_(type),
      testConfig_{EdenConfig::createTestEdenConfig()} {
  if (type != InodeCatalogType::Legacy) {
    inodeCatalog_ = std::make_unique<SqliteInodeCatalog>(
        tmpDirPath_ + "overlay"_pc, std::make_shared<NullStructuredLogger>());
  } else {
    inodeCatalog_ = std::make_unique<FsInodeCatalog>(&fcs_);
  }
}

void TestOverlay::recreateSqliteInodeCatalog() {
  if (type_ != InodeCatalogType::Legacy) {
    inodeCatalog_ = std::make_unique<SqliteInodeCatalog>(
        tmpDirPath_ + "overlay"_pc, std::make_shared<NullStructuredLogger>());
  }
}

TestDir TestOverlay::init() {
  auto nextInodeNumber =
      inodeCatalog_->initOverlay(/*createIfNonExisting=*/true);
  if (type_ != InodeCatalogType::Legacy) {
    fcs_.initialize(/*createIfNonExisting=*/true);
  }
  XCHECK(nextInodeNumber.has_value());
  XCHECK_GT(nextInodeNumber.value(), kRootNodeId);
  nextInodeNumber_ = nextInodeNumber.value().get();
  return TestDir(shared_from_this(), kRootNodeId);
}

// A simple class to create a basic directory & file structure in the overlay,
// and store references to various directory & file overlay state.
class SimpleOverlayLayout {
 public:
  explicit SimpleOverlayLayout(TestDir& root) : root_(&root) {
    // Save directory state to the overlay.
    // The order doesn't really matter here, as long as we save each of them
    // after their contents have been fully populated by the constructors below.
    root.save();
    src.save();
    src_foo.save();
    src_foo_x.save();
    src_foo_x_y.save();
    src_foo_x_y_sub.save();
    test.save();
    test_a.save();
    test_a_subdir.save();
    test_a_subdir_dir2.save();
  }

  TestDir* root_{nullptr};
  // src/: materialized
  TestDir src{root_->mkdir("src")};
  // src/readme.txt: non-materialized
  TestFile src_readmeTxt{src.create("readme.txt", "readme\n", makeTestId("1"))};
  // src/todo.txt: materialized
  TestFile src_todoTxt{src.create("todo.txt", "write tests\n")};
  // src/foo/: materialized
  TestDir src_foo{src.mkdir("foo")};
  // src/foo/test.txt: materialized
  TestFile src_foo_testTxt{src_foo.create("test.txt", "just some test data\n")};
  // src/foo/bar.txt: non-materialized
  TestFile src_foo_barTxt{
      src_foo.create("bar.txt", "not-materialized\n", makeTestId("1111"))};
  // src/foo/x/: materialized
  TestDir src_foo_x{src_foo.mkdir("x")};
  // src/foo/x/y/: materialized
  TestDir src_foo_x_y{src_foo_x.mkdir("y")};
  // src/foo/x/y/z.txt: materialized
  TestFile src_foo_x_y_zTxt{src_foo_x_y.create("z.txt", "zzz")};
  // src/foo/x/y/abc.txt: materialized
  TestFile src_foo_x_y_abcTxt{src_foo_x_y.create("abc.txt", "this is abc\n")};
  // src/foo/x/y/def.txt: materialized
  TestFile src_foo_x_y_defTxt{src_foo_x_y.create("def.txt", "this is def\n")};
  // src/foo/x/y/sub: materialized
  TestDir src_foo_x_y_sub{src_foo_x_y.mkdir("sub")};
  // src/foo/x/y/sub/xxx.txt: materialized
  TestFile src_foo_x_y_sub_xxxTxt{src_foo_x_y_sub.create("xxx.txt", "x y z")};
  // test/: non-materialized, present in overlay
  TestDir test{root_->mkdir("test", makeTestId("1234"))};
  // test/a/: non-materialized, present in overlay
  TestDir test_a{test.mkdir("a", makeTestId("5678"))};
  // test/b.txt: non-materialized
  TestFile test_bTxt{test.create("b.txt", "b contents\n", makeTestId("9abc"))};
  // test/a/subdir/: non-materialized, present in overlay
  TestDir test_a_subdir{test_a.mkdir("subdir", makeTestId("abcd"))};
  // test/a/subdir/dir1/: non-materialized, not present in overlay
  TestDir test_a_subdir_dir1{test_a_subdir.mkdir("dir1", makeTestId("a"))};
  // test/a/subdir/dir2/: non-materialized, present in overlay
  TestDir test_a_subdir_dir2{test_a_subdir.mkdir("dir2", makeTestId("b"))};
  // test/a/subdir/dir3/: non-materialized, not present in overlay
  TestDir test_a_subdir_dir3{test_a_subdir.mkdir("dir3", makeTestId("c"))};
  // test/a/subdir/file1 non-materialized
  TestFile test_a_subdir_file1{
      test_a_subdir.create("file1", "1\n", makeTestId("d"))};
  // test/a/subdir/file2 non-materialized
  TestFile test_a_subdir_file2{
      test_a_subdir.create("file2", "2\n", makeTestId("e"))};
};

std::vector<string> errorMessages(OverlayChecker& checker) {
  std::vector<string> results;
  for (const auto& err : checker.getErrors()) {
    results.push_back(err->getMessage(&checker));
  }
  return results;
}

std::string readFileContents(const AbsolutePath& path) {
  return readFile(path).value();
}

std::string readFsckLog(const OverlayChecker::RepairResult& result) {
  auto logPath = result.repairDir + "fsck.log"_pc;
  auto contents = readFileContents(logPath);
  XLOGF(DBG4, "fsck log {}:\n{}", logPath, contents);
  return contents;
}

std::pair<OverlayChecker::RepairResult, std::string> performRepair(
    OverlayChecker& checker,
    size_t expectedErrors,
    size_t expectedFixedErrors) {
  auto result = checker.repairErrors();
  if (!result.has_value()) {
    throw std::runtime_error("expected repairErrors() to find errors");
  }
  EXPECT_EQ(expectedErrors, result->totalErrors);
  EXPECT_EQ(expectedFixedErrors, result->fixedErrors);

  auto logContents = readFsckLog(*result);
  EXPECT_THAT(logContents, HasSubstr("Beginning fsck repair"));
  return std::make_pair(*result, logContents);
}

std::string readLostNFoundFile(
    const OverlayChecker::RepairResult& result,
    InodeNumber number,
    StringPiece suffix) {
  auto archivePath = result.repairDir + "lost+found"_pc +
      PathComponent(fmt::to_string(number.get())) + RelativePathPiece(suffix);
  return readFileContents(archivePath);
}

} // namespace

class FsckTest : public ::testing::TestWithParam<InodeCatalogType> {
 protected:
  InodeCatalogType overlayType() const {
    return GetParam();
  }
};

TEST_P(FsckTest, testNoErrors) {
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);
  testOverlay->closeCleanly();

  testOverlay->recreateSqliteInodeCatalog();
  FsFileContentStore& fcs = testOverlay->fcs();
  InodeCatalog* catalog = testOverlay->inodeCatalog();
  std::optional<InodeNumber> nextInode;
  if (overlayType() == InodeCatalogType::Legacy) {
    nextInode = catalog->initOverlay(/*createIfNonExisting=*/false);
  } else {
    nextInode = catalog->initOverlay(/*createIfNonExisting=*/true);
    fcs.initialize(/*createIfNonExisting=*/false);
  }
  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      catalog,
      &fcs,
      nextInode,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_EQ(0, checker.getErrors().size());
  EXPECT_THAT(errorMessages(checker), UnorderedElementsAre());

  // Test path computation
  EXPECT_EQ("src", checker.computePath(layout.src.number()).toString());
  EXPECT_EQ(
      "src/foo/x/y/z.txt",
      checker.computePath(layout.src_foo_x_y_zTxt.number()).toString());
  EXPECT_EQ(
      "src/foo/x/y/z.txt",
      checker
          .computePath(
              layout.src_foo_x_y.number(), layout.src_foo_x_y_zTxt.number())
          .toString());
  EXPECT_EQ(
      "src/foo/x/y/another_child.txt",
      checker.computePath(layout.src_foo_x_y.number(), "another_child.txt"_pc)
          .toString());
}

TEST_P(FsckTest, testMissingNextInodeNumber) {
  // This test is not applicable for Sqlite and InMemory backed overlays since
  // they implicitly track the next inode number
  if (overlayType() == InodeCatalogType::Sqlite ||
      overlayType() == InodeCatalogType::InMemory) {
    return;
  }
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);
  // Close the overlay without saving the next inode number
  testOverlay->inodeCatalog()->close(std::nullopt);

  FsFileContentStore& fcs = testOverlay->fcs();
  InodeCatalog* catalog = testOverlay->inodeCatalog();
  auto nextInode = catalog->initOverlay(/*createIfNonExisting=*/false);
  // Confirm there is no next inode data
  EXPECT_FALSE(nextInode.has_value());
  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      catalog,
      &fcs,
      nextInode,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  // OverlayChecker should still report 0 errors in this case.
  // We don't report a missing next inode number as an error: if this is the
  // only problem there isn't really anything to repair, so we don't want to
  // generate an fsck report.  The correct next inode number will always be
  // written out the next time we close the overlay.
  EXPECT_THAT(errorMessages(checker), UnorderedElementsAre());
  catalog->close(checker.getNextInodeNumber());
}

TEST_P(FsckTest, testBadNextInodeNumber) {
  // This test is not applicable for SQLite and InMemory backed overlays since
  // they implicitly track the next inode number
  if (overlayType() == InodeCatalogType::Sqlite ||
      overlayType() == InodeCatalogType::InMemory) {
    return;
  }
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);
  auto actualNextInodeNumber = testOverlay->getNextInodeNumber();
  // Use a bad next inode number when we close
  ASSERT_LE(2, actualNextInodeNumber.get());
  testOverlay->inodeCatalog()->close(InodeNumber(2));

  FsFileContentStore& fcs = testOverlay->fcs();
  InodeCatalog* catalog = testOverlay->inodeCatalog();
  auto nextInode = catalog->initOverlay(/*createIfNonExisting=*/false);
  EXPECT_EQ(2, nextInode ? nextInode->get() : 0);
  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      catalog,
      &fcs,
      nextInode,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(fmt::format(
          "bad stored next inode number: read 2 but should be at least {}",
          actualNextInodeNumber)));
  EXPECT_EQ(checker.getNextInodeNumber(), actualNextInodeNumber);
  catalog->close(checker.getNextInodeNumber());
}

TEST_P(FsckTest, testBadFileData) {
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);

  // Replace the data file for a file inode with a bogus header
  std::string badHeader(FsFileContentStore::kHeaderLength, 0x55);
  testOverlay->corruptInodeHeader(layout.src_foo_testTxt.number(), badHeader);

  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      testOverlay->inodeCatalog(),
      &testOverlay->fcs(),
      std::nullopt,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(fmt::format(
          "error reading data for inode {}: unknown overlay file format version {}",
          layout.src_foo_testTxt.number(),
          0x55555555)));

  // Repair the problems
  auto [result, fsckLog] = performRepair(checker, 1, 1);
  EXPECT_THAT(fsckLog, HasSubstr("1 problems detected"));
  EXPECT_THAT(fsckLog, HasSubstr("successfully repaired all 1 problems"));

  // Verify that the inode file for src/foo/test.txt was moved to the
  // lost+found directory.
  auto inodeContents =
      readLostNFoundFile(result, kRootNodeId, "src/foo/test.txt");
  EXPECT_EQ(badHeader + "just some test data\n", inodeContents);

  // Make sure the overlay now has a valid empty file at the same inode number
  auto replacementFile = testOverlay->fcs().openFile(
      layout.src_foo_testTxt.number(),
      FsFileContentStore::kHeaderIdentifierFile);
  std::array<std::byte, 128> buf;
  auto bytesRead = folly::readFull(
      std::get<folly::File>(replacementFile).fd(), buf.data(), buf.size());
  EXPECT_EQ(0, bytesRead);

  testOverlay->inodeCatalog()->close(checker.getNextInodeNumber());
}

TEST_P(FsckTest, testTruncatedDirData) {
  // This test doesn't work for SQLite or InMemory backed overlays because it
  // directly manipluates the written overlay data on disk to simulate file
  // corruption, which is not applicable for sqlite backed overlays
  if (overlayType() == InodeCatalogType::Sqlite ||
      overlayType() == InodeCatalogType::InMemory) {
    return;
  }
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);

  // Truncate one of the directory inode files to 0 bytes
  auto srcDataFile = testOverlay->fcs().openFileNoVerify(layout.src.number());
  folly::checkUnixError(
      ftruncate(std::get<folly::File>(srcDataFile).fd(), 0), "truncate failed");

  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      testOverlay->inodeCatalog(),
      &testOverlay->fcs(),
      std::nullopt,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(
          fmt::format(
              "error reading data for inode {}: file was too short to contain overlay header: "
              "read 0 bytes, expected 64 bytes",
              layout.src.number()),
          fmt::format(
              "found orphan directory inode {}", layout.src_foo.number()),
          fmt::format(
              "found orphan file inode {}", layout.src_todoTxt.number())));

  // Test path computation for one of the orphaned inodes
  EXPECT_EQ(
      fmt::format(
          "[unlinked({})]/x/y/another_child.txt", layout.src_foo.number()),
      checker.computePath(layout.src_foo_x_y.number(), "another_child.txt"_pc)
          .toString());

  // Repair the problems
  auto [result, fsckLog] = performRepair(checker, 3, 3);
  EXPECT_THAT(fsckLog, HasSubstr("3 problems detected"));
  EXPECT_THAT(fsckLog, HasSubstr("successfully repaired all 3 problems"));

  // The "src" directory that we removed contained 2 materialized children.
  // Make sure they were copied out to lost+found successfully.
  EXPECT_EQ(
      "write tests\n",
      readLostNFoundFile(result, layout.src_todoTxt.number(), ""));
  EXPECT_EQ(
      "just some test data\n",
      readLostNFoundFile(result, layout.src_foo.number(), "test.txt"));
  EXPECT_EQ(
      "zzz", readLostNFoundFile(result, layout.src_foo.number(), "x/y/z.txt"));

  // Make sure the overlay now has a valid empty directory where src/ was
  auto newDirContents =
      testOverlay->inodeCatalog()->loadOverlayDir(layout.src.number());
  ASSERT_TRUE(newDirContents.has_value());
  EXPECT_EQ(0, newDirContents->entries()->size());

  // No inodes from the orphaned subtree should be present in the
  // overlay any more.
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_readmeTxt.number()));
  EXPECT_FALSE(testOverlay->fcs().hasOverlayFile(layout.src_todoTxt.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_testTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_barTxt.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo_x.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo_x_y.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_zTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_abcTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_defTxt.number()));

  testOverlay->inodeCatalog()->close(checker.getNextInodeNumber());
}

TEST_P(FsckTest, testMissingDirData) {
  // This test doesn't work for SQLite or InMemory backed overlays because it
  // directly manipluates the written overlay metadata on disk to simulate
  // file corruption, which is not applicable for sqlite backed overlays
  if (overlayType() == InodeCatalogType::Sqlite ||
      overlayType() == InodeCatalogType::InMemory) {
    return;
  }
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);

  // Remove the overlay file for the "src/" directory
  testOverlay->inodeCatalog()->removeOverlayDir(layout.src.number());
  // To help fully exercise the code that copies orphan subtrees to lost+found,
  // also corrupt the file for "src/foo/test.txt", which will need to be copied
  // out as part of the orphaned src/ children subdirectories.  This makes sure
  // the orphan repair logic also handles corrupt files in the orphan subtree.
  std::string badHeader(FsFileContentStore::kHeaderLength, 0x55);
  testOverlay->corruptInodeHeader(layout.src_foo_testTxt.number(), badHeader);
  // And remove the "src/foo/x" subdirectory that is also part of the orphaned
  // subtree.
  testOverlay->inodeCatalog()->removeOverlayDir(layout.src_foo_x.number());

  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      testOverlay->inodeCatalog(),
      &testOverlay->fcs(),
      std::nullopt,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(
          fmt::format(
              "missing overlay file for materialized directory inode {} (src)",
              layout.src.number()),
          fmt::format(
              "found orphan directory inode {}", layout.src_foo.number()),
          fmt::format(
              "found orphan file inode {}", layout.src_todoTxt.number()),
          fmt::format(
              "missing overlay file for materialized directory inode {} ([unlinked({})]/x)",
              layout.src_foo_x.number(),
              layout.src_foo.number()),
          fmt::format(
              "found orphan directory inode {}", layout.src_foo_x_y.number()),
          fmt::format(
              "error reading data for inode {}: unknown overlay file format version {}",
              layout.src_foo_testTxt.number(),
              0x55555555)));

  // Repair the problems
  auto [result, fsckLog] = performRepair(checker, 6, 6);
  EXPECT_THAT(fsckLog, HasSubstr("6 problems detected"));
  EXPECT_THAT(fsckLog, HasSubstr("successfully repaired all 6 problems"));

  // The "src" directory that we removed contained some materialized children.
  // Make sure they were copied out to lost+found successfully.
  EXPECT_EQ(
      "write tests\n",
      readLostNFoundFile(result, layout.src_todoTxt.number(), ""));
  EXPECT_EQ(
      badHeader + "just some test data\n",
      readLostNFoundFile(result, layout.src_foo.number(), "test.txt"));
  EXPECT_EQ(
      "zzz", readLostNFoundFile(result, layout.src_foo_x_y.number(), "z.txt"));
  EXPECT_EQ(
      "x y z",
      readLostNFoundFile(result, layout.src_foo_x_y.number(), "sub/xxx.txt"));

  // Make sure the overlay now has a valid empty directory where src/ was
  auto newDirContents =
      testOverlay->inodeCatalog()->loadOverlayDir(layout.src.number());
  ASSERT_TRUE(newDirContents.has_value());
  EXPECT_EQ(0, newDirContents->entries()->size());

  // No inodes from the orphaned subtree should be present in the
  // overlay any more.
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_readmeTxt.number()));
  EXPECT_FALSE(testOverlay->fcs().hasOverlayFile(layout.src_todoTxt.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_testTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_barTxt.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo_x.number()));
  EXPECT_FALSE(
      testOverlay->inodeCatalog()->hasOverlayDir(layout.src_foo_x_y.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_zTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_abcTxt.number()));
  EXPECT_FALSE(
      testOverlay->fcs().hasOverlayFile(layout.src_foo_x_y_defTxt.number()));

  testOverlay->inodeCatalog()->close(checker.getNextInodeNumber());
}

TEST_P(FsckTest, testHardLink) {
  auto testOverlay = make_shared<TestOverlay>(overlayType());
  auto root = testOverlay->init();
  SimpleOverlayLayout layout(root);
  // Add an entry to src/foo/x/y/z.txt in src/foo
  layout.src_foo.linkFile(layout.src_foo_x_y_zTxt.number(), "also_z.txt");
  layout.src_foo.save();

  InodeCatalog::LookupCallback lookup = [](auto&&, auto&&) {
    return makeImmediateFuture<InodeCatalog::LookupCallbackValue>(
        std::runtime_error("no lookup callback"));
  };
  OverlayChecker checker(
      testOverlay->inodeCatalog(),
      &testOverlay->fcs(),
      std::nullopt,
      lookup,
      testOverlay->getTestConfig()->fsckNumErrorDiscoveryThreads.getValue());
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(fmt::format(
          "found hard linked inode {}:\n- src/foo/also_z.txt\n- src/foo/x/y/z.txt",
          layout.src_foo_x_y_zTxt.number())));
  testOverlay->inodeCatalog()->close(checker.getNextInodeNumber());
}

INSTANTIATE_TEST_SUITE_P(
    FsckTest,
    FsckTest,
    ::testing::Values(
        InodeCatalogType::Legacy,
        InodeCatalogType::Sqlite,
        InodeCatalogType::InMemory));
