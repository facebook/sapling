/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <memory>

#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>

#include "eden/fs/inodes/overlay/FsOverlay.h"
#include "eden/fs/inodes/overlay/OverlayChecker.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::ByteRange;
using folly::StringPiece;
using std::make_shared;
using std::make_unique;
using std::string;
using ::testing::UnorderedElementsAre;

namespace {

class TestDir;

class TestOverlay : public std::enable_shared_from_this<TestOverlay> {
 public:
  TestOverlay();

  /*
   * Initialize the TestOverlay object.
   *
   * Returns the root directory.
   */
  TestDir init();

  const AbsolutePath& path() const {
    return tmpDirPath_;
  }

  FsOverlay& fs() {
    return fs_;
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
    fs_.close(getNextInodeNumber());
  }

 private:
  folly::test::TemporaryDirectory tmpDir_;
  AbsolutePath tmpDirPath_;
  FsOverlay fs_;
  uint64_t nextInodeNumber_{0};
};

class TestFile {
 public:
  TestFile(
      std::shared_ptr<TestOverlay> overlay,
      InodeNumber number,
      folly::File&& file)
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
      std::optional<Hash> hash = std::nullopt,
      mode_t permissions = 0755) {
    auto number = addEntry(name, hash, S_IFDIR | (permissions & 07777));
    return TestDir(overlay_, number);
  }

  TestDir linkFile(
      InodeNumber number,
      StringPiece name,
      std::optional<Hash> hash = std::nullopt,
      mode_t permissions = 0755) {
    addEntry(name, hash, S_IFREG | (permissions & 07777), number.get());
    return TestDir(overlay_, number);
  }

  TestFile create(
      StringPiece name,
      ByteRange contents,
      std::optional<Hash> hash = std::nullopt,
      mode_t permissions = 0644) {
    auto number = addEntry(name, hash, S_IFREG | (permissions & 07777));
    // The file should only be created in the overlay if it is materialized
    folly::File file;
    if (!hash.has_value()) {
      file = overlay_->fs().createOverlayFile(number, contents);
    }
    return TestFile(overlay_, number, std::move(file));
  }

  TestFile create(
      StringPiece name,
      StringPiece contents,
      std::optional<Hash> hash = std::nullopt,
      mode_t permissions = 0644) {
    return create(name, ByteRange(contents), hash, permissions);
  }

  void save() {
    overlay_->fs().saveOverlayDir(number_, contents_);
  }

 private:
  InodeNumber addEntry(
      StringPiece name,
      std::optional<Hash> hash,
      mode_t mode,
      uint64_t number = 0) {
    auto insertResult =
        contents_.entries.emplace(name, overlay::OverlayEntry{});
    if (!insertResult.second) {
      throw std::runtime_error(
          folly::to<string>("an entry named \"", name, "\" already exists"));
    }

    if (number == 0) {
      number = overlay_->allocateInodeNumber().get();
    }
    auto& entry = insertResult.first->second;
    entry.mode = mode;
    entry.inodeNumber = static_cast<int64_t>(number);
    if (hash) {
      auto hashBytes = hash->getBytes();
      entry.set_hash(std::string{
          reinterpret_cast<const char*>(hashBytes.data()), hashBytes.size()});
    }
    return InodeNumber(number);
  }

  std::shared_ptr<TestOverlay> overlay_;
  InodeNumber number_;
  overlay::OverlayDir contents_;
};

TestOverlay::TestOverlay()
    : tmpDir_(makeTempDir()),
      tmpDirPath_(tmpDir_.path().string()),
      fs_(tmpDirPath_) {}

TestDir TestOverlay::init() {
  auto nextInodeNumber = fs_.initOverlay(/*createIfNonExisting=*/true);
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
    test.save();
    test_a.save();
    test_a_subdir.save();
    test_a_subdir_dir2.save();
  }

  TestDir* root_{nullptr};
  // src/: materialized
  TestDir src{root_->mkdir("src")};
  // src/readme.txt: non-materialized
  TestFile src_readmeTxt{
      src.create("readme.txt", "readme\n", makeTestHash("1"))};
  // src/todo.txt: materialized
  TestFile src_todoTxt{src.create("todo.txt", "write tests\n")};
  // src/foo/: materialized
  TestDir src_foo{src.mkdir("foo")};
  // src/foo/test.txt: materialized
  TestFile src_foo_testTxt{src_foo.create("test.txt", "just some test data\n")};
  // src/foo/bar.txt: non-materialized
  TestFile src_foo_barTxt{
      src_foo.create("bar.txt", "not-materialized\n", makeTestHash("1111"))};
  // src/foo/x/: materialized
  TestDir src_foo_x{src_foo.mkdir("x")};
  // src/foo/x/y/: materialized
  TestDir src_foo_x_y{src_foo_x.mkdir("y")};
  // src/foo/x/y/z.txt: materialized
  TestFile src_foo_x_y_zTxt{src_foo_x_y.create("z.txt", "zzz")};
  // test/: non-materialized, present in overlay
  TestDir test{root_->mkdir("test", makeTestHash("1234"))};
  // test/a/: non-materialized, present in overlay
  TestDir test_a{test.mkdir("a", makeTestHash("5678"))};
  // test/b.txt: non-materialized
  TestFile test_bTxt{
      test.create("b.txt", "b contents\n", makeTestHash("9abc"))};
  // test/a/subdir/: non-materialized, present in overlay
  TestDir test_a_subdir{test_a.mkdir("subdir", makeTestHash("abcd"))};
  // test/a/subdir/dir1/: non-materialized, not present in overlay
  TestDir test_a_subdir_dir1{test_a_subdir.mkdir("dir1", makeTestHash("a"))};
  // test/a/subdir/dir2/: non-materialized, present in overlay
  TestDir test_a_subdir_dir2{test_a_subdir.mkdir("dir2", makeTestHash("b"))};
  // test/a/subdir/dir3/: non-materialized, not present in overlay
  TestDir test_a_subdir_dir3{test_a_subdir.mkdir("dir3", makeTestHash("c"))};
  // test/a/subdir/file1 non-materialized
  TestFile test_a_subdir_file1{
      test_a_subdir.create("file1", "1\n", makeTestHash("d"))};
  // test/a/subdir/file2 non-materialized
  TestFile test_a_subdir_file2{
      test_a_subdir.create("file2", "2\n", makeTestHash("e"))};
};

std::vector<string> errorMessages(OverlayChecker& checker) {
  std::vector<string> results;
  for (const auto& err : checker.getErrors()) {
    results.push_back(err->getMessage(&checker));
  }
  return results;
}

} // namespace

TEST(Fsck, testNoErrors) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);
  overlay->closeCleanly();

  FsOverlay fs(overlay->path());
  auto nextInode = fs.initOverlay(/*createIfNonExisting=*/false);
  OverlayChecker checker(&fs, nextInode);
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

TEST(Fsck, testMissingNextInodeNumber) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);
  // Close the overlay without saving the next inode number
  overlay->fs().close(std::nullopt);

  FsOverlay fs(overlay->path());
  auto nextInode = fs.initOverlay(/*createIfNonExisting=*/false);
  // Confirm there is no next inode data
  EXPECT_FALSE(nextInode.has_value());
  OverlayChecker checker(&fs, nextInode);
  checker.scanForErrors();
  // OverlayChecker should still report 0 errors in this case.
  // We don't report a missing next inode number as an error: if this is the
  // only problem there isn't really anything to repair, so we don't want to
  // generate an fsck report.  The correct next inode number will always be
  // written out the next time we close the overlay.
  EXPECT_THAT(errorMessages(checker), UnorderedElementsAre());
  fs.close(checker.getNextInodeNumber());
}

TEST(Fsck, testBadNextInodeNumber) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);
  auto actualNextInodeNumber = overlay->getNextInodeNumber();
  // Use a bad next inode number when we close
  ASSERT_LE(2, actualNextInodeNumber.get());
  overlay->fs().close(InodeNumber(2));

  FsOverlay fs(overlay->path());
  auto nextInode = fs.initOverlay(/*createIfNonExisting=*/false);
  EXPECT_EQ(2, nextInode ? nextInode->get() : 0);
  OverlayChecker checker(&fs, nextInode);
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(folly::to<string>(
          "bad stored next inode number: read 2 but should be at least ",
          actualNextInodeNumber)));
  EXPECT_EQ(checker.getNextInodeNumber(), actualNextInodeNumber);
  fs.close(checker.getNextInodeNumber());
}

TEST(Fsck, testTruncatedDirData) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);

  // Truncate one of the directory inode files to 0 bytes
  auto srcDataFile = overlay->fs().openFileNoVerify(layout.src.number());
  folly::checkUnixError(ftruncate(srcDataFile.fd(), 0), "truncate failed");

  OverlayChecker checker(&overlay->fs(), std::nullopt);
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(
          folly::to<string>(
              "error reading data for inode ",
              layout.src.number(),
              ": file was too short to contain overlay header: "
              "read 0 bytes, expected 64 bytes"),
          folly::to<string>("found orphan inode ", layout.src_foo.number()),
          folly::to<string>(
              "found orphan inode ", layout.src_todoTxt.number())));

  // Test path computation for one of the orphaned inodes
  EXPECT_EQ(
      folly::to<string>(
          "[unlinked(", layout.src_foo.number(), ")]/x/y/another_child.txt"),
      checker.computePath(layout.src_foo_x_y.number(), "another_child.txt"_pc)
          .toString());

  overlay->fs().close(checker.getNextInodeNumber());
}

TEST(Fsck, testMissingDirData) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);

  // Remove one of the directory overlay files
  overlay->fs().removeOverlayFile(layout.src.number());

  OverlayChecker checker(&overlay->fs(), std::nullopt);
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(
          folly::to<string>(
              "missing overlay file for materialized directory inode ",
              layout.src.number(),
              " (src)"),
          folly::to<string>("found orphan inode ", layout.src_foo.number()),
          folly::to<string>(
              "found orphan inode ", layout.src_todoTxt.number())));
  overlay->fs().close(checker.getNextInodeNumber());
}

TEST(Fsck, testHardLink) {
  auto overlay = make_shared<TestOverlay>();
  auto root = overlay->init();
  SimpleOverlayLayout layout(root);
  // Add an entry to src/foo/x/y/z.txt in src/foo
  layout.src_foo.linkFile(layout.src_foo_x_y_zTxt.number(), "also_z.txt");
  layout.src_foo.save();

  OverlayChecker checker(&overlay->fs(), std::nullopt);
  checker.scanForErrors();
  EXPECT_THAT(
      errorMessages(checker),
      UnorderedElementsAre(folly::to<string>(
          "found hard linked inode ",
          layout.src_foo_x_y_zTxt.number(),
          ":\n",
          "- src/foo/also_z.txt\n",
          "- src/foo/x/y/z.txt")));
  overlay->fs().close(checker.getNextInodeNumber());
}
