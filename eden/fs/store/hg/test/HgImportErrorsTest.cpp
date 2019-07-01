/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/Conv.h>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/dynamic.h>
#include <folly/experimental/TestUtil.h>
#include <folly/json.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <gtest/gtest.h>
#include <optional>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/tracing/EdenStats.h"

using namespace facebook::eden;
using namespace facebook::eden::path_literals;
using namespace std::chrono_literals;
using folly::dynamic;
using folly::File;
using namespace folly::literals;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
using std::make_shared;
using std::make_unique;
using std::optional;
using std::string;

DEFINE_string(
    fakeHgImportHelper,
    "",
    "The path to the fake_hg_import_helper test script");

namespace {

constexpr auto kTimeout = 10s;

class HgImportErrorTest : public ::testing::Test {
 protected:
  struct ManifestEntry {
    ManifestEntry(StringPiece pathArg, StringPiece flagsArg, Hash hashArg)
        : path{pathArg.str()}, flags{flagsArg.str()}, hash{hashArg} {}

    std::string path;
    std::string flags;
    Hash hash;
  };
  struct ManifestInfo {
    ManifestInfo(Hash hash, std::vector<ManifestEntry> entriesArg)
        : id{hash}, entries(std::move(entriesArg)) {}

    Hash id;
    std::vector<ManifestEntry> entries;
  };
  struct BlobInfo {
    BlobInfo(StringPiece pathArg, Hash revHashArg, StringPiece contentsArg)
        : path{pathArg.str()},
          revHash{revHashArg},
          contents{contentsArg.str()} {}

    std::string path;
    Hash revHash;
    std::string contents;
  };

  void defineBlob(StringPiece path, Hash revHash, StringPiece contents) {
    blobs_.emplace_back(path, revHash, contents);
  }
  void defineManifest(Hash id, std::vector<ManifestEntry> entries) {
    manifests_.emplace_back(id, std::move(entries));
  }

  void writeData() {
    dynamic jsonManifests = dynamic::object();
    for (const auto& manifest : manifests_) {
      dynamic jsonEntries = dynamic::array();
      for (const auto& entry : manifest.entries) {
        auto jsonEntry =
            dynamic::array(entry.path, entry.flags, entry.hash.toString());
        jsonEntries.push_back(jsonEntry);
      }
      jsonManifests.insert(manifest.id.toString(), jsonEntries);
    }

    dynamic jsonBlobs = dynamic::object();
    for (const auto& blob : blobs_) {
      auto key = folly::to<string>(blob.path, ":", blob.revHash.toString());
      jsonBlobs.insert(key, blob.contents);
    }
    dynamic jsonData =
        dynamic::object("manifests", jsonManifests)("blobs", jsonBlobs);

    auto dataPath = testPath_ + "data.json"_pc;
    File dataFile(dataPath.value(), O_CREAT | O_WRONLY, 0644);
    auto fileContents = folly::toPrettyJson(jsonData);
    auto ret = folly::writeFull(
        dataFile.fd(), fileContents.data(), fileContents.size());
    folly::checkUnixError(ret, "error writing test data file");
  }

  void triggerError(StringPiece key, StringPiece error) {
    // Write out a file telling fake_hg_import_helper.py to force an error
    auto path = testPath_ + PathComponentPiece(key);
    File file(path.value(), O_CREAT | O_WRONLY, 0644);
    auto ret = folly::writeFull(file.fd(), error.data(), error.size());
    folly::checkUnixError(ret, "error writing error trigger file for ", key);
  }
  void triggerBlobError(StringPiece path, Hash revHash, StringPiece error) {
    auto key = folly::to<string>("error.blob.", path, ":", revHash.toString());
    for (char& c : key) {
      if (c == '/') {
        c = '_';
      }
    }
    triggerError(key, error);
  }
  void triggerManifestError(Hash rev, StringPiece error) {
    triggerError(folly::to<string>("error.manifest.", rev.toString()), error);
  }

  AbsolutePath findFakeImportHelperPath();

  template <typename ImporterType = HgImporterManager>
  void createStore() {
    // Some of the tests call createStore() more than once (to reset state in
    // the middle of the test).  Explicitly destroy backingStore_ before we do
    // this.  HgBackingStore unfortunately keeps some global state (the
    // threadLocalImporter that manages thread-local state) and we want to make
    // sure this gets reset before trying to create a new HgBackingStore that is
    // also using the current thread as its importer thread.
    objectStore_.reset();
    backingStore_.reset();

    auto fakeImportHelper = findFakeImportHelperPath();
    XLOG(DBG2) << "found fake hg_import_helper at " << fakeImportHelper;

    writeData();

    localStore_ = make_shared<MemoryLocalStore>();

    importer_ = make_unique<ImporterType>(
        testPath_,
        localStore_.get(),
        getSharedHgImporterStatsForCurrentThread(stats_),
        fakeImportHelper);
    backingStore_ =
        make_shared<HgBackingStore>(importer_.get(), localStore_.get(), stats_);
    objectStore_ = ObjectStore::create(localStore_, backingStore_, stats_);
  }

  template <typename ImporterType>
  void testBlobError(
      StringPiece errorType,
      optional<StringPiece> errorRegex = std::nullopt);

  template <typename ImporterType>
  void testBlobError(StringPiece errorType, StringPiece errorRegex) {
    testBlobError<ImporterType>(errorType, optional<StringPiece>(errorRegex));
  }

  std::vector<BlobInfo> blobs_;
  std::vector<ManifestInfo> manifests_;
  TemporaryDirectory testDir_{"eden_hg_import_test"};
  AbsolutePath testPath_{testDir_.path().string()};

  std::unique_ptr<Importer> importer_;
  std::shared_ptr<MemoryLocalStore> localStore_;
  std::shared_ptr<HgBackingStore> backingStore_;
  std::shared_ptr<ObjectStore> objectStore_;
  std::shared_ptr<EdenStats> stats_ = std::make_shared<EdenStats>();
};

AbsolutePath HgImportErrorTest::findFakeImportHelperPath() {
  // If a path was specified on the command line, use that
  if (!FLAGS_fakeHgImportHelper.empty()) {
    return realpath(FLAGS_fakeHgImportHelper);
  }

  const char* argv0 = gflags::GetArgv0();
  if (argv0 == nullptr) {
    throw std::runtime_error(
        "unable to find hg_import_helper.py script: "
        "unable to determine edenfs executable path");
  }

  auto programPath = realpath(argv0);
  XLOG(DBG4) << "edenfs path: " << programPath;
  auto programDir = programPath.dirname();

  auto isHelper = [](const AbsolutePath& path) {
    XLOG(DBG8) << "checking for hg_import_helper at \"" << path << "\"";
    return access(path.value().c_str(), X_OK) == 0;
  };

  // Now check in all parent directories of the directory containing our
  // binary.  This is where we will find the helper program if we are running
  // from the build output directory in a source code repository.
  AbsolutePathPiece dir = programDir;
  RelativePathPiece helperPath{
      "eden/fs/store/hg/test/fake_hg_import_helper.par"};
  while (true) {
    auto path = dir + helperPath;
    if (isHelper(path)) {
      return path;
    }
    auto parent = dir.dirname();
    if (parent == dir) {
      throw std::runtime_error(
          "unable to find fake_hg_import_helper.par script");
    }
    dir = parent;
  }
}
} // namespace

#define EXPECT_BLOB_EQ(blob, data) \
  EXPECT_EQ((blob)->getContents().clone()->moveToFbString(), (data))

// A simple sanity test to ensure the fake_hg_import_helper.py script
// works as expected when returning successful responses.
TEST_F(HgImportErrorTest, testNoErrors) {
  defineBlob("foo/abc.c", makeTestHash("5678"), "abc.c v 5678");
  defineBlob("foo/bar.txt", makeTestHash("1234"), "bar.txt v 1234");
  defineManifest(
      makeTestHash("abcdef"),
      {
          ManifestEntry("foo/abc.c", "", makeTestHash("5678")),
          ManifestEntry("foo/bar.txt", "", makeTestHash("1234")),
      });
  createStore();

  auto rootTree =
      objectStore_->getTreeForCommit(makeTestHash("abcdef")).get(kTimeout);
  auto fooEntry = rootTree->getEntryPtr("foo"_pc);
  ASSERT_TRUE(fooEntry);
  auto fooTree = objectStore_->getTree(fooEntry->getHash()).get(kTimeout);
  auto barEntry = fooTree->getEntryPtr("bar.txt"_pc);
  ASSERT_TRUE(barEntry);

  auto bar = objectStore_->getBlob(barEntry->getHash()).get(kTimeout);
  EXPECT_BLOB_EQ(bar, "bar.txt v 1234");
}

template <typename ImporterType>
void HgImportErrorTest::testBlobError(
    StringPiece errorType,
    optional<StringPiece> errorMsg) {
  defineBlob("foo/abc.c", makeTestHash("5678"), "abc.c v 5678");
  defineBlob("foo/bar.txt", makeTestHash("1234"), "bar.txt v 1234");
  defineManifest(
      makeTestHash("abcdef"),
      {
          ManifestEntry("foo/abc.c", "", makeTestHash("5678")),
          ManifestEntry("foo/bar.txt", "", makeTestHash("1234")),
      });
  createStore<ImporterType>();

  auto rootTree =
      objectStore_->getTreeForCommit(makeTestHash("abcdef")).get(kTimeout);
  auto fooEntry = rootTree->getEntryPtr("foo"_pc);
  ASSERT_TRUE(fooEntry);
  auto fooTree = objectStore_->getTree(fooEntry->getHash()).get(kTimeout);
  auto barEntry = fooTree->getEntryPtr("bar.txt"_pc);
  ASSERT_TRUE(barEntry);

  // The HgImporterManager code should retry once, so a single crash from the
  // import helper script should still result in a successful import.
  triggerBlobError("foo/bar.txt", makeTestHash("1234"), errorType);

  std::shared_ptr<const Blob> bar;
  try {
    bar = objectStore_->getBlob(barEntry->getHash()).get(kTimeout);
  } catch (const std::exception& ex) {
    if (!errorMsg.has_value()) {
      FAIL() << "unexpected error during blob import: "
             << folly::exceptionStr(ex);
    }
    StringPiece actualMsg(ex.what());
    if (!actualMsg.contains(errorMsg.value())) {
      FAIL() << "blob import failed with unexpected error message: "
             << folly::exceptionStr(ex);
    }
    return;
  }
  EXPECT_FALSE(errorMsg.has_value())
      << "blob import succeeded unexpectedly: "
      << "expecting error message matching \"" << errorMsg.value() << "\"";
  EXPECT_BLOB_EQ(bar, "bar.txt v 1234");
}

TEST_F(HgImportErrorTest, testBlobImportCrashOnce) {
  // Using HgImporter directly should fail if the CMD_CAT_FILE call fails
  testBlobError<HgImporter>("exit_once", "received unexpected EOF"_sp);
  testBlobError<HgImporter>(
      "bad_txn_once", "received unexpected transaction ID"_sp);
}

TEST_F(HgImportErrorTest, testBlobImportManagerCrashOnce) {
  // Using HgImporterManager will retry once on error, so a single error
  // should be transparently hidden.
  testBlobError<HgImporterManager>("exit_once");
  testBlobError<HgImporterManager>("bad_txn_once");
}

TEST_F(HgImportErrorTest, testBlobImportManagerPersistentCrash) {
  // Using HgImporterManager will fail if the import helper fails more than once
  testBlobError<HgImporterManager>("exit", "received unexpected EOF"_sp);
  testBlobError<HgImporterManager>(
      "bad_txn", "received unexpected transaction ID"_sp);
}
