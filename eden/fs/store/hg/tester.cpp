/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/Hash.h"

#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/init/Init.h>
#include <folly/io/Cursor.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <rocksdb/db.h>
#include <rocksdb/utilities/options_util.h>
#include <sysexits.h>
#include <optional>

#include "eden/fs/model/Tree.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgImporter.h"
#include "eden/fs/store/hg/HgManifestImporter.h"
#include "eden/fs/utils/PathFuncs.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(rev, "", "The revision ID to import");
DEFINE_string(
    import_type,
    "flat",
    "The hg import mechanism to use: \"flat\" or \"tree\"");
DEFINE_string(
    subdir,
    "",
    "A subdirectory to import when using --import_type=tree.");
DEFINE_string(
    rocksdb_options_file,
    "",
    "A path to a rocksdb options file to use when creating a "
    "temporary rocksdb");
DEFINE_bool(
    tree_import_recurse,
    true,
    "Recursively import all trees under the specified subdirectory when "
    "performing a treemanifest import");

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::Endian;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Cursor;
using folly::test::TemporaryDirectory;
using std::string;

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2");

namespace {

std::unique_ptr<rocksdb::DB> createRocksDb(AbsolutePathPiece dbPath) {
  rocksdb::Options options;
  if (FLAGS_rocksdb_options_file.empty()) {
    options.IncreaseParallelism();
    options.OptimizeLevelStyleCompaction();
  } else {
    std::vector<rocksdb::ColumnFamilyDescriptor> cfDescs;
    auto env = rocksdb::Env::Default();
    auto status = rocksdb::LoadOptionsFromFile(
        FLAGS_rocksdb_options_file, env, &options, &cfDescs);
    if (!status.ok()) {
      throw std::runtime_error(
          folly::to<string>("Failed to load DB options: ", status.ToString()));
    }
    fprintf(
        stderr,
        "loaded rocksdb options from %s\n",
        FLAGS_rocksdb_options_file.c_str());
  }

  options.create_if_missing = true;

  // Open DB.
  rocksdb::DB* db;
  auto status = rocksdb::DB::Open(options, dbPath.stringPiece().str(), &db);
  if (!status.ok()) {
    throw std::runtime_error(
        folly::to<string>("Failed to open DB: ", status.ToString()));
  }

  return std::unique_ptr<rocksdb::DB>(db);
}

void importTreeRecursive(
    HgBackingStore* store,
    RelativePathPiece path,
    const Tree* tree) {
  for (const auto& entry : tree->getTreeEntries()) {
    if (entry.isTree()) {
      auto entryPath = path + entry.getName();
      std::unique_ptr<Tree> subtree;
      try {
        subtree = store->getTree(entry.getHash()).get();
      } catch (const std::exception& ex) {
        printf(
            "** error importing tree %s: %s\n",
            entryPath.stringPiece().str().c_str(),
            ex.what());
        continue;
      }
      printf(
          "  Recursively imported \"%s\"\n",
          entryPath.stringPiece().str().c_str());
      importTreeRecursive(store, entryPath, subtree.get());
    }
  }
}

#if EDEN_HAVE_HG_TREEMANIFEST
int importTree(
    LocalStore* store,
    AbsolutePathPiece repoPath,
    StringPiece revName,
    RelativePath subdir) {
  UnboundedQueueExecutor resultThreadPool(1, "ResultThread");
  HgBackingStore backingStore(repoPath, store, &resultThreadPool, nullptr);

  printf(
      "Importing revision \"%s\" using tree manifest\n", revName.str().c_str());
  auto rootHash = backingStore.importTreeManifest(Hash(revName)).get();
  printf("/: %s\n", rootHash.toString().c_str());

  auto tree = store->getTree(rootHash).get(10s);
  for (const auto& component : subdir.components()) {
    auto entry = tree->getEntryPtr(component);
    if (!entry) {
      printf("%s: not found\n", component.stringPiece().str().c_str());
      return EX_DATAERR;
    }
    if (!entry->isTree()) {
      printf("%s: not a tree\n", component.stringPiece().str().c_str());
      return EX_DATAERR;
    }
    printf(
        "%s: %s\n",
        component.stringPiece().str().c_str(),
        entry->getHash().toString().c_str());
    tree = backingStore.getTree(entry->getHash()).get();
  }

  if (FLAGS_tree_import_recurse) {
    importTreeRecursive(&backingStore, subdir, tree.get());
  }

  return EX_OK;
}
#endif // EDEN_HAVE_HG_TREEMANIFEST
} // namespace

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);

  if (argc != 2) {
    fprintf(stderr, "usage: hg_import <repository>\n");
    return EX_USAGE;
  }
  auto repoPath = realpath(argv[1]);

  std::optional<TemporaryDirectory> tmpDir;
  AbsolutePath rocksPath;
  if (FLAGS_edenDir.empty()) {
    tmpDir = TemporaryDirectory("eden_hg_tester");
    rocksPath = AbsolutePath{tmpDir->path().string()};
    createRocksDb(rocksPath);
  } else {
    if (!FLAGS_rocksdb_options_file.empty()) {
      fprintf(
          stderr,
          "error: --edenDir and --rocksdb_options_file are incompatible\n");
      return EX_USAGE;
    }
    rocksPath = canonicalPath(FLAGS_edenDir) + "storage/rocks-db"_relpath;
  }

  std::string revName = FLAGS_rev;
  if (revName.empty()) {
    revName = ".";
  }

  RocksDbLocalStore store(rocksPath);

  int returnCode = EX_OK;
  if (FLAGS_import_type == "flat") {
    HgImporter importer(repoPath, &store);
    printf("Importing revision \"%s\" using flat manifest\n", revName.c_str());
    auto rootHash = importer.importFlatManifest(revName);
    printf("Imported root tree: %s\n", rootHash.toString().c_str());
  } else if (FLAGS_import_type == "tree") {
#if EDEN_HAVE_HG_TREEMANIFEST
    RelativePath path{FLAGS_subdir};
    returnCode = importTree(&store, repoPath, revName, path);
#else // !EDEN_HAVE_HG_TREEMANIFEST
    fprintf(
        stderr, "error: treemanifest import is not supported by this build\n");
    return EX_UNAVAILABLE;
#endif // EDEN_HAVE_HG_TREEMANIFEST
  } else {
    fprintf(
        stderr,
        "error: unknown import type \"%s\"; must be \"flat\" or \"tree\"\n",
        FLAGS_import_type.c_str());
    return EX_USAGE;
  }

  return returnCode;
}
