/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <algorithm>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include <folly/Range.h>
#include <folly/testing/TestUtil.h>

#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/fuzz/overlay/OverlayFileFuzzFixture.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/lmdbcatalog/LMDBFileContentStore.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "security/lionhead/utils/lib_ftest/ftest.h"

namespace {

using namespace facebook::eden;

/**
 * One LMDB content store per process, the store Overlay pairs with
 * MemInodeCatalog (InodeCatalogType::InMemory) and LMDBInodeCatalog. Opening
 * an LMDB environment is a directory of files plus a lock, so as with the
 * Legacy fixture it is paid once rather than per iteration; each iteration
 * removes the blob it wrote.
 */
class LmdbFileContentStoreFixture {
 public:
  LmdbFileContentStoreFixture()
      : tmpDir_{makeTempDir("eden_overlay_lmdb_fuzz")},
        store_{
            canonicalPath(tmpDir_.path().string()),
            std::make_shared<EdenFsEventsLogger>(nullptr)} {
    store_.initialize(/*createIfNonExisting=*/true);
  }

  ~LmdbFileContentStoreFixture() {
    store_.close();
  }

  LmdbFileContentStoreFixture(const LmdbFileContentStoreFixture&) = delete;
  LmdbFileContentStoreFixture& operator=(const LmdbFileContentStoreFixture&) =
      delete;
  LmdbFileContentStoreFixture(LmdbFileContentStoreFixture&&) = delete;
  LmdbFileContentStoreFixture& operator=(LmdbFileContentStoreFixture&&) =
      delete;

  static LmdbFileContentStoreFixture& global() {
    static LmdbFileContentStoreFixture fixture;
    return fixture;
  }

  LMDBFileContentStore& store() {
    return store_;
  }

 private:
  folly::test::TemporaryDirectory tmpDir_;
  LMDBFileContentStore store_;
};

folly::ByteRange asRange(const std::vector<uint8_t>& bytes) {
  return folly::ByteRange{bytes.data(), bytes.size()};
}

} // namespace

extern "C" int LionheadFuzzerCustomInitialize(int*, char***) {
  LmdbFileContentStoreFixture::global();
  return 0;
}

namespace {

/**
 * The remaining input is a file inode's contents, written through
 * LMDBFileContentStore::createOverlayFile (the ByteRange overload, one iovec
 * into LMDBStoreInterface::saveBlob's MDB_RESERVE + memcpy loop) and read
 * back. An empty input is the interesting case: a materialized empty file
 * hands saveBlob an iovec with a null base and a zero length.
 */
FUZZ(OverlayLmdb, create_overlay_file) {
  auto& store = LmdbFileContentStoreFixture::global().store();
  auto ino = OverlayFileFuzzFixture::pickInode(f.u8("inode_selector"));
  auto contents = f.all_remaining_bytes();
  if (contents.size() > kMaxFuzzedOverlayFileSize) {
    contents.resize(kMaxFuzzedOverlayFileSize);
  }

  store.createOverlayFile(ino, asRange(contents));
  auto readBack = store.readOverlayFile(ino);
  store.removeOverlayFile(ino);

  FTEST_CHECK_EQ(readBack.size(), contents.size());
  FTEST_CHECK(
      std::equal(
          contents.begin(),
          contents.end(),
          reinterpret_cast<const uint8_t*>(readBack.data())));
}

} // namespace
