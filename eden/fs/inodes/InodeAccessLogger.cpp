/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeAccessLogger.h"

#include <folly/Random.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>

#include <re2/re2.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/LogEvent.h"

namespace facebook::eden {

namespace {
constexpr folly::StringPiece kHgDirectory{".hg"};
constexpr folly::StringPiece kEdenDirectory{".eden"};

constexpr folly::StringPiece kFbsource{"fbsource"};
// TODO(helsel): make this configurable in EdenConfig
const static RE2 kFbsourceFilter("xplat\\/toolchains\\/minimal_xcode");
} // namespace

InodeAccessLogger::InodeAccessLogger(
    std::shared_ptr<ReloadableConfig> reloadableConfig,
    std::shared_ptr<StructuredLogger> structuredLogger)
    : reloadableConfig_{std::move(reloadableConfig)},
      structuredLogger_{std::move(structuredLogger)} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("InodeAccessLoggerProcessor");
    processInodeAccessEvents();
  }};
}

InodeAccessLogger::~InodeAccessLogger() {
  state_.wlock()->workerThreadShouldStop = true;
  // Do one final post here to ensure the thread wakes up, sees that
  // workerThreadShouldStop, and returns from processInodeAccessEvents().
  // Otherwise, the workerThread_ would be stuck waiting on the semaphore in
  // the infinite work loop in processInodeAccessEvents and would never join
  sem_.post();
  workerThread_.join();
  XLOG(INFO, "InodeAccessLogger shut down");
}

bool InodeAccessLogger::filterDirectory(
    folly::StringPiece directory,
    folly::StringPiece repo) {
  // Don't log events from the .eden or .hg directories. startsWith has an
  // empty check so we should not go out of bounds here.
  if (directory.startsWith(kEdenDirectory) ||
      directory.startsWith(kHgDirectory)) {
    return true;
  }

  if (repo == kFbsource) {
    // Use PartialMatch and avoid a trailing .*
    return RE2::PartialMatch(directory.str(), kFbsourceFilter);
  }

  return false;
}

void InodeAccessLogger::processInodeAccessEvents() {
  std::vector<InodeAccess> work;

  for (;;) {
    work.clear();

    sem_.wait();

    {
      auto state = state_.wlock();
      if (state->workerThreadShouldStop) {
        // We may lose log events here if the work queue is not empty, but
        // these events are not important enough to block shutdown.
        return;
      }

      work.swap(state->work);
    }

    // logInodeAccess posts for every event added to the work queue, but we wait
    // on the semaphore only once per batch of events. For example, we could
    // post multiple times before this single wait, and we will pull and process
    // all the events on the queue for just a single wait. This makes the
    // semaphore more positive than it needs to be and is a performance cost of
    // extra spinning if left unaddressed.  sem_.wait() consumed one count, but
    // we know this semaphore was posted work.size() amount of times. Since we
    // will process all entries at once, rather than waking repeatedly, consume
    // the rest.
    if (!work.empty()) {
      // The - 1 here is to account for the initial semaphore wait. For example,
      // if only one event was added to the queue and the wait() was fulfilled,
      // work.size() would be 1, and we would not wait to try any extra waits,
      // so the -1 brings this to 0.
      (void)sem_.tryWait(work.size() - 1);
    }

    for (auto& event : work) {
      folly::StringPiece repo;
      std::optional<RelativePath> path;
      try {
        if (auto mount = event.edenMount.lock()) {
          auto repo_optional =
              mount->getObjectStore()->getBackingStore()->getRepoName();
          if (repo_optional == std::nullopt) {
            XLOG(
                DBG5,
                "InodeAccessLogger couldn't get repo name from backing store");
            continue;
          }
          repo = repo_optional.value();
          path = mount->getInodeMap()->getPathForInode(event.inodeNumber);
        } else {
          // The pointer has expired, just continue to the next event
          continue;
        }
      } catch (const std::exception& ex) {
        // getMount can throw if the mount is not known to the EdenFS instance
        // or if it is not safe for inode access.
        // getPathForInode can throw if the inode is invalid. Since we
        // process these events in an async queue, it is possible that the
        // inode is invalidated before we get to it. In this case, just
        // continue to the next event.
        XLOGF_EVERY_MS(
            WARN, 30000, "Error looking up inode path: {}", ex.what());
        path = std::nullopt;
      }

      // Don't log if the path does not exist or if getPathForInode threw an
      // error
      if (path == std::nullopt) {
        continue;
      }

      dtype_t dtype = event.dtype;
      std::string directory;

      if (dtype == dtype_t::Dir) {
        directory = path->asString();
      } else {
        directory = path->dirname().asString();
      }

      // Check if this directory matches one of the in place global or repo
      // specific filters
      if (filterDirectory(directory, repo)) {
        continue;
      }

      // This will be empty if denominator != 1, we only log filename if we're
      // logging 100% of file accesses.
      std::string filename;

      // Use a configurable percentage to determine if we should log the sample.
      auto denominator = reloadableConfig_->getEdenConfig()
                             ->logFileAccessesSamplingDenominator.getValue();

      // Only log the filename if we're logging 100% of file accesses and the
      // path is not to a directory
      if (denominator == 1 && dtype != dtype_t::Dir) {
        filename = path->basename().asString();
      } else {
        // If we're not logging filenames, and the directory is empty (meaning
        // this was a top level file access), lets just not log it.
        if (directory.empty()) {
          continue;
        }
      }

      // TODO: Don't log files that match gitignore rules.

      // We check our percentage after we've passed all of our filtering and
      // have determined that this a sample we'd actually log.
      if (0 != folly::Random::rand32(denominator)) {
        continue;
      }

      std::string source;
      switch (event.cause) {
        case ObjectFetchContext::Unknown:
          source = "unknown";
          break;
        case ObjectFetchContext::Fs:
          source = "fs";
          break;
        case ObjectFetchContext::Thrift:
          source = "thrift";
          break;
        case ObjectFetchContext::Prefetch:
          source = "prefetch";
          break;
      }

      std::string sourceDetail;
      if (event.causeDetail.has_value()) {
        sourceDetail = event.causeDetail.value();
      }

      // TODO: Instead of logging every access individually, store the events in
      // a memory bounded LRU cache and coalesce the events by directory,
      // source, source_detail, and filename (if applicable). Then, a periodic
      // task can flush this cache to Scuba. This would make it possible to
      // increase the InodeAccessesPercentage without overwhelming Scribe.

      structuredLogger_->logEvent(FileAccessEvent{
          repo.str(),
          std::move(directory),
          std::move(filename),
          std::move(source),
          std::move(sourceDetail)});
    }
  }
}

void InodeAccessLogger::logInodeAccess(InodeAccess access) {
  if (!reloadableConfig_->getEdenConfig()->logFileAccesses.getValue()) {
    return;
  }
  auto state = state_.wlock();
  state->work.push_back(std::move(access));
  sem_.post();
}
} // namespace facebook::eden
