/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenServiceHandler.h"

#include <boost/polymorphic_cast.hpp>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/Subprocess.h>
#include <folly/futures/Future.h>
#include <unordered_set>
#include "EdenError.h"
#include "EdenServer.h"
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/EdenMountHandler.h"
#include "eden/fs/service/GlobNode.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

using std::make_unique;
using std::string;
using std::unique_ptr;
using std::vector;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;

namespace facebook {
namespace eden {

EdenServiceHandler::EdenServiceHandler(EdenServer* server)
    : FacebookBase2("Eden"), server_(server) {}

facebook::fb303::cpp2::fb_status EdenServiceHandler::getStatus() {
  return facebook::fb303::cpp2::fb_status::ALIVE;
}

void EdenServiceHandler::mount(std::unique_ptr<MountInfo> info) {
  try {
    mountImpl(*info);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::mountImpl(const MountInfo& info) {
  server_->reloadConfig();
  auto initialConfig = ClientConfig::loadFromClientDirectory(
      AbsolutePathPiece{info.mountPoint},
      AbsolutePathPiece{info.edenClientPath},
      server_->getConfig().get());

  auto repoType = initialConfig->getRepoType();
  auto backingStore =
      server_->getBackingStore(repoType, initialConfig->getRepoSource());
  auto objectStore =
      make_unique<ObjectStore>(server_->getLocalStore(), backingStore);
  auto edenMount =
      EdenMount::makeShared(std::move(initialConfig), std::move(objectStore));
  // We gave ownership of initialConfig to the EdenMount.
  // Get a pointer to it that we can use for the remainder of this function.
  auto* config = edenMount->getConfig();

  // Load InodeBase objects for any materialized files in this mount point
  // before we start mounting.
  edenMount->getRootInode()->loadMaterializedChildren().wait();

  // TODO(mbolin): Use the result of config.getBindMounts() to perform the
  // appropriate bind mounts for the client.
  server_->mount(edenMount);

  auto cloneSuccessPath = config->getCloneSuccessPath();
  bool isInitialMount = access(cloneSuccessPath.c_str(), F_OK) != 0;
  if (isInitialMount) {
    auto repoHooks = config->getRepoHooks();
    auto postCloneScript = repoHooks + RelativePathPiece("post-clone");
    auto repoSource = config->getRepoSource();

    LOG(INFO) << "Running post-clone hook '" << postCloneScript << "' for "
              << info.mountPoint;
    try {
      // TODO(mbolin): It would be preferable to pass the name of the repository
      // as defined in ~/.edenrc so that the script can derive the repoType and
      // repoSource from that. Then the hook would only take two args.
      folly::Subprocess proc(
          {postCloneScript.c_str(), repoType, info.mountPoint, repoSource},
          folly::Subprocess::pipeStdin());
      proc.closeParentFd(STDIN_FILENO);
      proc.waitChecked();
      LOG(INFO) << "Finished post-clone hook '" << postCloneScript << "' for "
                << info.mountPoint;
    } catch (const folly::SubprocessSpawnError& ex) {
      // If this failed because postCloneScript does not exist, then ignore the
      // error because we are tolerant of the case where /etc/eden/hooks does
      // not exist, by design.
      if (ex.errnoValue() != ENOENT) {
        // TODO(13448173): If clone fails, then we should roll back the mount.
        throw;
      }
      LOG(INFO) << "Did not run post-clone hook '" << postCloneScript
                << "' for " << info.mountPoint << " because it was not found.";
    }
  }

  // The equivalent of `touch` to signal that clone completed successfully.
  folly::writeFile(string(), cloneSuccessPath.c_str());
}

/**
 * The path to the metadata for this mount is available at
 * ~/.eden/clients/CLIENT_HASH.
 */
AbsolutePath EdenServiceHandler::getPathToDirstateStorage(
    AbsolutePathPiece mountPointPath) {
  // We need to take the sha-1 of the utf-8 version of path.
  folly::ByteRange bytes(mountPointPath.stringPiece());
  auto sha1 = Hash::sha1(bytes);
  auto component = PathComponent(sha1.toString());

  return server_->getEdenDir() + PathComponent("clients") + component +
      PathComponent("dirstate");
}

void EdenServiceHandler::unmount(std::unique_ptr<std::string> mountPoint) {
  try {
    server_->unmount(*mountPoint);
  } catch (const EdenError& ex) {
    throw;
  } catch (const std::exception& ex) {
    throw newEdenError(ex);
  }
}

void EdenServiceHandler::listMounts(std::vector<MountInfo>& results) {
  for (const auto& edenMount : server_->getMountPoints()) {
    MountInfo info;
    info.mountPoint = edenMount->getPath().stringPiece().str();
    // TODO: Fill in info.edenClientPath.
    // I'll add that in a future diff, once we have a custom MountPoint
    // subclass that isn't in the low-level fusell namespace.
    results.push_back(info);
  }
}

void EdenServiceHandler::checkOutRevision(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> hash) {
  Hash hashObj(*hash);
  AbsolutePathPiece mountPointForClient(*mountPoint);

  auto edenMount = server_->getMount(*mountPoint);
  if (!edenMount) {
    throw EdenError("requested mount point is not known to this eden instance");
  }

  auto root = edenMount->getRootInode();
  CHECK_NOTNULL(root.get());

  root->performCheckout(hashObj);
}

void EdenServiceHandler::getSHA1(
    vector<SHA1Result>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> paths) {
  vector<Future<Hash>> futures;
  for (const auto& path : *paths) {
    futures.emplace_back(getSHA1ForPathDefensively(*mountPoint, path));
  }

  auto results = folly::collectAll(std::move(futures)).get();
  for (auto& result : results) {
    out.emplace_back();
    SHA1Result& sha1Result = out.back();
    if (result.hasValue()) {
      sha1Result.set_sha1(StringPiece{result.value().getBytes()}.str());
    } else {
      sha1Result.set_error(newEdenError(result.exception()));
    }
  }
}

Future<Hash> EdenServiceHandler::getSHA1ForPathDefensively(
    StringPiece mountPoint,
    StringPiece path) noexcept {
  // Calls getSHA1ForPath() and traps all immediate exceptions and converts
  // them in to a Future result.
  try {
    return getSHA1ForPath(mountPoint, path);
  } catch (const std::system_error& e) {
    return makeFuture<Hash>(newEdenError(e));
  }
}

Future<Hash> EdenServiceHandler::getSHA1ForPath(
    StringPiece mountPoint,
    StringPiece path) {
  if (path.empty()) {
    return makeFuture<Hash>(
        newEdenError(EINVAL, "path cannot be the empty string"));
  }

  auto edenMount = server_->getMount(mountPoint);
  auto relativePath = RelativePathPiece{path};
  return edenMount->getInode(relativePath).then([](const InodePtr& inode) {
    auto fileInode = inode.asFilePtr();
    if (!S_ISREG(fileInode->getEntry()->mode)) {
      // We intentionally want to refuse to compute the SHA1 of symlinks
      return makeFuture<Hash>(
          InodeError(EINVAL, fileInode, "file is a symlink"));
    }
    return fileInode->getSHA1();
  });
}

void EdenServiceHandler::getMaterializedEntries(
    MaterializedResult& out,
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);
  if (!edenMount) {
    throw newEdenError(ENODEV, "no such mount point \"{}\"", *mountPoint);
  }

  return getMaterializedEntriesForMount(edenMount.get(), out);
}

void EdenServiceHandler::getBindMounts(
    std::vector<string>& out,
    std::unique_ptr<string> mountPointPtr) {
  auto mountPoint = *mountPointPtr.get();
  auto mountPointPath = AbsolutePathPiece{mountPoint};
  auto edenMount = server_->getMount(mountPoint);

  for (auto& bindMount : edenMount->getBindMounts()) {
    out.emplace_back(mountPointPath.relativize(bindMount.pathInMountDir)
                         .stringPiece()
                         .str());
  }
}

void EdenServiceHandler::getCurrentJournalPosition(
    JournalPosition& out,
    std::unique_ptr<std::string> mountPoint) {
  auto edenMount = server_->getMount(*mountPoint);
  auto latest = edenMount->getJournal().rlock()->getLatest();

  out.mountGeneration = edenMount->getMountGeneration();
  out.sequenceNumber = latest->toSequence;
  out.snapshotHash = StringPiece(latest->toHash.getBytes()).str();
}

void EdenServiceHandler::getFilesChangedSince(
    FileDelta& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<JournalPosition> fromPosition) {
  auto edenMount = server_->getMount(*mountPoint);
  auto delta = edenMount->getJournal().rlock()->getLatest();

  if (fromPosition->mountGeneration != edenMount->getMountGeneration()) {
    throw newEdenError(
        ERANGE,
        "fromPosition.mountGeneration does not match the current "
        "mountGeneration.  "
        "You need to compute a new basis for delta queries.");
  }

  std::unordered_set<RelativePath> changedFiles;

  out.toPosition.sequenceNumber = delta->toSequence;
  out.toPosition.snapshotHash = StringPiece(delta->toHash.getBytes()).str();
  out.toPosition.mountGeneration = edenMount->getMountGeneration();

  out.fromPosition = out.toPosition;

  while (delta) {
    if (delta->toSequence <= fromPosition->sequenceNumber) {
      // We've reached the end of the interesting section
      break;
    }

    changedFiles.insert(
        delta->changedFilesInOverlay.begin(),
        delta->changedFilesInOverlay.end());

    out.fromPosition.sequenceNumber = delta->fromSequence;
    out.fromPosition.snapshotHash =
        StringPiece(delta->fromHash.getBytes()).str();
    out.fromPosition.mountGeneration = edenMount->getMountGeneration();

    delta = delta->previous;
  }

  for (auto& path : changedFiles) {
    out.paths.emplace_back(path.stringPiece().str());
  }
}

void EdenServiceHandler::getFileInformation(
    std::vector<FileInformationOrError>& out,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths) {
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  for (auto& path : *paths) {
    FileInformationOrError result;

    try {
      auto relativePath = RelativePathPiece{path};
      auto inodeBase = edenMount->getInodeBlocking(relativePath);

      // we've reached the item of interest.
      auto attr = inodeBase->getattr().get();
      FileInformation info;
      info.size = attr.st.st_size;
      info.mtime.seconds = attr.st.st_mtim.tv_sec;
      info.mtime.nanoSeconds = attr.st.st_mtim.tv_nsec;
      info.mode = attr.st.st_mode;

      result.set_info(info);
      out.emplace_back(std::move(result));

    } catch (const std::system_error& e) {
      result.set_error(newEdenError(e));
      out.emplace_back(std::move(result));
    }
  }
}

void EdenServiceHandler::glob(
    vector<string>& out,
    unique_ptr<string> mountPoint,
    unique_ptr<vector<string>> globs) {
  auto edenMount = server_->getMount(*mountPoint);
  auto rootInode = edenMount->getRootInode();

  // Compile the list of globs into a tree
  GlobNode globRoot;
  for (auto& globString : *globs) {
    globRoot.parse(globString);
  }

  // and evaluate it against the root
  auto matches = globRoot.evaluate(RelativePathPiece(), rootInode).get();
  for (auto& fileName : matches) {
    out.emplace_back(fileName.stringPiece().toString());
  }
}

void EdenServiceHandler::scmGetStatus(
    ThriftHgStatus& out,
    std::unique_ptr<std::string> mountPoint) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  auto status = dirstate->getStatus();
  auto& entries = out.entries;
  for (auto& pair : *status->list()) {
    auto statusCode = pair.second;
    entries[pair.first.stringPiece().str()] = statusCode;
  }
}

void EdenServiceHandler::scmAdd(
    std::vector<ScmAddRemoveError>& errorsToReport,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  std::vector<RelativePathPiece> relativePaths;
  for (auto& path : *paths.get()) {
    relativePaths.emplace_back(path);
  }
  std::vector<DirstateAddRemoveError> dirstateErrorsToReport;
  dirstate->addAll(relativePaths, &dirstateErrorsToReport);
  for (auto& error : dirstateErrorsToReport) {
    errorsToReport.emplace_back();
    errorsToReport.back().path = error.path.stringPiece().str();
    errorsToReport.back().errorMessage = error.errorMessage;
  }
}

void EdenServiceHandler::scmRemove(
    std::vector<ScmAddRemoveError>& errorsToReport,
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::vector<std::string>> paths,
    bool force) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  std::vector<RelativePathPiece> relativePaths;
  for (auto& path : *paths.get()) {
    relativePaths.emplace_back(path);
  }
  std::vector<DirstateAddRemoveError> dirstateErrorsToReport;
  dirstate->removeAll(relativePaths, force, &dirstateErrorsToReport);
  for (auto& error : dirstateErrorsToReport) {
    errorsToReport.emplace_back();
    errorsToReport.back().path = error.path.stringPiece().str();
    errorsToReport.back().errorMessage = error.errorMessage;
  }
}

namespace {
/**
 * Because a 20-byte hash is declared as "binary" in a .thrift file, which
 * becomes a std::unique_ptr<std::string> when turned into a C++ parameter, this
 * provides a convenience method for converting the std::string into a Hash.
 */
Hash createHashForCommitID(const std::string* commitID) {
  return Hash(folly::ByteRange(folly::StringPiece(*commitID)));
}
}

void EdenServiceHandler::scmMarkCommitted(
    std::unique_ptr<std::string> mountPoint,
    std::unique_ptr<std::string> commitID,
    std::unique_ptr<std::vector<std::string>> pathsToCleanAsStrings,
    std::unique_ptr<std::vector<std::string>> pathsToDropAsStrings) {
  auto dirstate = server_->getMount(*mountPoint)->getDirstate();
  DCHECK(dirstate != nullptr) << "Failed to get dirstate for "
                              << mountPoint.get();

  auto hash = createHashForCommitID(commitID.get());
  std::vector<RelativePathPiece> pathsToClean;
  pathsToClean.reserve(pathsToCleanAsStrings->size());
  for (auto& path : *pathsToCleanAsStrings.get()) {
    pathsToClean.emplace_back(RelativePathPiece(path));
  }

  std::vector<RelativePathPiece> pathsToDrop;
  pathsToDrop.reserve(pathsToDropAsStrings->size());
  for (auto& path : *pathsToDropAsStrings.get()) {
    pathsToDrop.emplace_back(RelativePathPiece(path));
  }

  dirstate->markCommitted(hash, pathsToClean, pathsToDrop);
}

void EdenServiceHandler::shutdown() {
  server_->stop();
}
}
} // facebook::eden
