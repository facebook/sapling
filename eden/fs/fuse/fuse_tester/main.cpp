/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/init/Init.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventBaseThread.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <signal.h>
#include <sysexits.h>
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/PrivHelperImpl.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/tracing/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessNameCache.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::exceptionStr;
using folly::makeFuture;
using std::string;

DEFINE_int32(numFuseThreads, 4, "The number of FUSE worker threads");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2,eden.fs.fuse=DBG7");

namespace {
class TestDispatcher : public Dispatcher {
 public:
  TestDispatcher(EdenStats* stats, const UserInfo& identity)
      : Dispatcher(stats), identity_(identity) {}

  folly::Future<Attr> getattr(InodeNumber ino) override {
    if (ino == kRootNodeId) {
      struct stat st = {};
      st.st_ino = ino.get();
      st.st_mode = S_IFDIR | 0755;
      st.st_nlink = 2;
      st.st_uid = identity_.getUid();
      st.st_gid = identity_.getGid();
      st.st_blksize = 512;
      st.st_blocks = 1;
      return folly::makeFuture(Attr(st, /* timeout */ 0));
    }
    folly::throwSystemErrorExplicit(ENOENT);
  }

  UserInfo identity_;
};

void ensureEmptyDirectory(AbsolutePathPiece path) {
  boost::filesystem::path boostPath(
      path.stringPiece().begin(), path.stringPiece().end());

  XLOG(INFO) << "boost path: " << boostPath.native();
  if (!boost::filesystem::create_directories(boostPath)) {
    // This directory already existed.  Make sure it is empty.
    if (!boost::filesystem::is_empty(boostPath)) {
      throw std::runtime_error(
          folly::to<string>(path, " does not refer to an empty directory"));
    }
  }
}
} // namespace

int main(int argc, char** argv) {
  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);
  if (argc != 2) {
    fprintf(stderr, "usage: test_mount PATH\n");
    return EX_NOPERM;
  }

  auto sigresult = signal(SIGPIPE, SIG_IGN);
  if (sigresult == SIG_ERR) {
    folly::throwSystemError("error ignoring SIGPIPE");
  }

  // Determine the desired user and group ID.
  if (geteuid() != 0) {
    fprintf(stderr, "error: fuse_tester must be started as root\n");
    return EX_NOPERM;
  }
  folly::checkPosixError(chdir("/"), "failed to chdir(/)");

  // Fork the privhelper process, then drop privileges.
  auto identity = UserInfo::lookup();
  auto privHelper = startPrivHelper(identity);
  identity.dropPrivileges();

  auto mountPath = normalizeBestEffort(argv[1]);
  try {
    ensureEmptyDirectory(mountPath);
  } catch (const std::exception& ex) {
    fprintf(stderr, "error with mount path: %s\n", exceptionStr(ex).c_str());
    return EX_DATAERR;
  }

  // For simplicity, start a separate EventBaseThread to drive the privhelper
  // I/O.  We only really need this for the initial fuseMount() call.  We could
  // run an EventBase in the current thread until the fuseMount() completes,
  // but using EventBaseThread is simpler for now.
  folly::EventBaseThread evbt;
  evbt.getEventBase()->runInEventBaseThreadAndWait(
      [&] { privHelper->attachEventBase(evbt.getEventBase()); });
  auto fuseDevice = privHelper->fuseMount(mountPath.value()).get(100ms);

  EdenStats stats;
  TestDispatcher dispatcher(&stats, identity);

  std::unique_ptr<FuseChannel, FuseChannelDeleter> channel(new FuseChannel(
      std::move(fuseDevice),
      mountPath,
      FLAGS_numFuseThreads,
      &dispatcher,
      std::make_shared<ProcessNameCache>()));

  XLOG(INFO) << "Starting FUSE...";
  auto completionFuture = channel->initialize().get();
  XLOG(INFO) << "FUSE started";

  auto stopData = std::move(completionFuture).get();
  XLOG(INFO) << "FUSE channel done; stop_reason="
             << static_cast<int>(stopData.reason);

  return EX_OK;
}
