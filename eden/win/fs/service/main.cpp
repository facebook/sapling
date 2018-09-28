/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/Conv.h>
#include <folly/experimental/FunctionScheduler.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <iostream>
#include <memory>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/win/fs/utils/StringConv.h"
#include "folly/io/IOBuf.h"
// DEFINE_bool(allowRoot, false, "Allow running eden directly as root");
// DEFINE_string(edenDir, "", "The path to the .eden directory");
// DEFINE_string(
//    etcEdenDir,
//    "/etc/eden",
//    "the directory holding all system configuration files");
// define_string(configpath, "", "the path of the ~/.edenrc config file");
// DEFINE_string(configPath, "", "The path of the ~/.edenrc config file");
// DEFINE_string(
//    logPath,
//    "if set, redirects stdout and stderr to the log file given.");

using namespace facebook::edenwin;
// using namespace facebook::eden;
using namespace std;
using namespace folly;

// Set the default log level for all eden logs to DBG2
// Also change the "default" log handler (which logs to stderr) to log
// messages asynchronously rather than blocking in the logging thread.
// FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

void debugSetLogLevel(std::string category, std::string level) {
  auto& db = folly::LoggerDB::get();
  db.getCategoryOrNull(category);
  folly::Logger(category).getCategory()->setLevel(
      folly::stringToLogLevel(level), true);
}

///////////////////////////////////////
// The following is temp code to test. This would go away.

#include "eden/fs/service/EdenCPUThreadPool.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/SqliteLocalStore.h"
#include "eden/fs/store/git/GitBackingStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using namespace facebook::eden;

constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kThriftSocketName{"socket"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kRocksDBPath{"storage\\rocks-db"};
constexpr StringPiece kSqlitePath{"storage\\sqlite.db"};

std::shared_ptr<LocalStore> localStore_;
std::shared_ptr<UnboundedQueueExecutor> threadPool_;
AbsolutePath edenDir_;
AbsolutePath etcEdenDir_;
AbsolutePath configPath_;

shared_ptr<BackingStore> backingStore_;
unique_ptr<ObjectStore> objectStore_;

shared_ptr<BackingStore> createBackingStore(
    StringPiece type,
    StringPiece name) {
  cout << "createBackingStore: type: " << type << " name: " << name << endl;
  if (type == "null") {
    // return make_shared<EmptyBackingStore>();
  } else if (type == "hg") {
    const auto repoPath = realpath(name);
    return make_shared<HgBackingStore>(
        repoPath, localStore_.get(), threadPool_.get());
    // Disabling git support in this test code.
    //} else if (type == "git") {
    //  throw std::domain_error(
    //      folly::to<string>("unsupported backing store type: ", type));
    //   const auto repoPath = realpath(name);
    //   return make_shared<GitBackingStore>(repoPath, localStore_.get());
  } else {
    throw std::domain_error(
        folly::to<string>("unsupported backing store type: ", type));
  }
}

void StartBackingStore() {
  edenDir_ = facebook::eden::realpath("c:\\eden\\eden");
  etcEdenDir_ = facebook::eden::realpath("c:\\eden\\etcedendir");
  configPath_ = facebook::eden::realpath("c:\\eden\\configpath\\.edenrc");

  printf("StartBackingStore\n");
  const auto path = edenDir_ + RelativePathPiece{kSqlitePath};
  XLOG(DBG2) << "opening local Sqlite store " << path;
  localStore_ = make_shared<SqliteLocalStore>(path);
  XLOG(DBG2) << "done opening local Sqlite store";

  threadPool_ = std::make_shared<EdenCPUThreadPool>();

  printf("CreateBackingStore\n");
  backingStore_ = createBackingStore("hg", "c:\\open\\fbsource");

  objectStore_ = std::make_unique<ObjectStore>(localStore_, backingStore_);

  // facebook::eden::Hash commitID("777362dde8e5");
  // facebook::eden::Hash commitID("777362dde8e574bda92c42816b7df0de0e8aba39");
  facebook::eden::Hash commitID("67f1923706e05421e823effbb51e41770486a5e0");
  // facebook::eden::Hash commitID("240625dabfa3b0b442e4939147de860d5a916459");
  Future<unique_ptr<const Tree>> futTree =
      backingStore_->getTreeForCommit(commitID);

  unique_ptr<const Tree> tree = futTree.get();
  cout << "TREE ENTRIES";

  for (const auto& entry : tree->getTreeEntries()) {
    cout << entry.getName() << endl;
  }
}
/////////////////////////////////

int __cdecl main(int argc, char** argv) {
  cout << "Eden Windows - started" << endl;

  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);
  debugSetLogLevel("eden", "DBG");
  debugSetLogLevel(".", "DBG");

  // std::wstring rootPath = argv[1];
  wstring rootPath = L"virtfs";

  XLOG(INFO) << "Mounting the virtual FS at"
             << StringConv::wstringToString(rootPath);

  StartBackingStore();
  // StartFS(rootPath);

  return 0;
};
