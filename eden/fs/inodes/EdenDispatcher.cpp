/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenDispatcher.h"

#include <dirent.h>
#include <wangle/concurrent/CPUThreadPoolExecutor.h>
#include <wangle/concurrent/GlobalExecutor.h>
#include <shared_mutex>
#include "EdenMount.h"
#include "FileHandle.h"
#include "FileInode.h"
#include "InodeMap.h"
#include "TreeInode.h"
#include "eden/fuse/DirHandle.h"
#include "eden/fuse/FileHandle.h"

using namespace folly;
using facebook::eden::PathComponentPiece;
using facebook::eden::PathComponent;
using facebook::eden::RelativePath;
using std::string;
using std::vector;

DEFINE_int32(
    inode_reserve,
    1000000,
    "pre-size inode hash table for this many entries");

DEFINE_bool(
    warm_kernel_on_startup,
    false,
    "whether to crawl ourselves on startup to warm up the kernel "
    "inode/vnode cache");

DEFINE_int32(
    warm_kernel_num_threads,
    32,
    "how many threads to use when crawling ourselves during warm up.  "
    "Making this too large without raising the file descriptors "
    "ulimit can cause serious problems and has diminishing returns on "
    "crawl performance.");

DEFINE_int32(
    warm_kernel_delay,
    1,
    "how many seconds to delay before triggering the inode/vnode cache warmup");

namespace facebook {
namespace eden {

EdenDispatcher::EdenDispatcher(EdenMount* mount)
    : mount_(mount), inodeMap_(mount_->getInodeMap()) {}

namespace {
/* We use this class to warm up the kernel inode/vnode cache after we've
 * mounted.
 * The time this takes for large trees can be rather significant, so it
 * is worthwhile to spend some effort to do this in parallel as soon as
 * we're mounted; it can reduce the wall time that the user will see
 * pretty significantly.
 */
struct Walker : public std::enable_shared_from_this<Walker> {
  std::atomic<uint32_t> nwalk{0};
  std::atomic<uint32_t> nfiles{0};
  std::string rootPath;
  std::chrono::steady_clock::time_point start;
  wangle::CPUThreadPoolExecutor pool;

  explicit Walker(const std::string& rootPath)
      : rootPath(rootPath),
        start(std::chrono::steady_clock::now()),
        pool(
            FLAGS_warm_kernel_num_threads,
            1 /* priorities */,
            FLAGS_inode_reserve /* max queue size */) {}

  void walk() {
    auto self = shared_from_this();
    std::thread thr([=] {
      sleep(FLAGS_warm_kernel_delay);
      LOG(INFO) << "Initiating walk of myself to warm up inode cache, use "
                   "--warm_kernel_on_startup=false to disable";
      self->walkDir(rootPath);
    });
    thr.detach();
  }

  void stop() {
    pool.stop();
  }

  void walkDir(const std::string& path) {
    auto self = shared_from_this();
    ++nwalk;
    via(&pool)
        .then([=] {
          struct stat st;
          if (lstat(path.c_str(), &st) != 0) {
            LOG(ERROR) << "failed to lstat(" << path
                       << "): " << strerror(errno);
            return;
          }
          ++nfiles;

          if (!S_ISDIR(st.st_mode)) {
            return;
          }
          auto dir = opendir(path.c_str());
          if (!dir) {
            LOG(ERROR) << "Failed to opendir(" << path
                       << "): " << strerror(errno);
            return;
          }
          SCOPE_EXIT {
            closedir(dir);
          };
          while (true) {
            auto de = readdir(dir);
            if (!de) {
              return;
            }
            if (strcmp(de->d_name, ".") == 0 || strcmp(de->d_name, "..") == 0) {
              continue;
            }
            auto full = folly::to<std::string>(path, "/", de->d_name);
            self->walkDir(full);
          }
        })
        .onError([](const std::exception& e) {
          LOG(ERROR) << "Error during walk: " << e.what();
        })
        .ensure([=] {
          if (--nwalk == 0) {
            LOG(INFO) << "Finished walking " << nfiles << " files, took "
                      << std::chrono::duration_cast<std::chrono::milliseconds>(
                             std::chrono::steady_clock::now() - start)
                             .count()
                      << "ms";
            // Since `self` owns the executor in which we're running,
            // we'll deadlock ourselves if we allow the destructor to
            // execute in one of its threads.  Switch to a different
            // context to shut down this pool
            wangle::getCPUExecutor()->add([self] { self->stop(); });
          }
        });
  }
};

/** Compute a fuse_entry_param */
fuse_entry_param computeEntryParam(
    fuse_ino_t number,
    const fusell::Dispatcher::Attr& attr) {
  fuse_entry_param entry;
  entry.ino = number;
  entry.generation = 1;
  entry.attr = attr.st;
  entry.attr_timeout = attr.timeout;
  entry.entry_timeout = attr.timeout;
  return entry;
}
}

void EdenDispatcher::initConnection(fuse_conn_info& /* conn */) {
  if (FLAGS_warm_kernel_on_startup) {
    auto walker =
        std::make_shared<Walker>(mount_->getPath().stringPiece().str());
    walker->walk();
  }
}

folly::Future<fusell::Dispatcher::Attr> EdenDispatcher::getattr(
    fuse_ino_t ino) {
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->getattr(); });
}

folly::Future<std::shared_ptr<fusell::DirHandle>> EdenDispatcher::opendir(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  return inodeMap_->lookupTreeInode(ino).then(
      [fi](const TreeInodePtr& inode) { return inode->opendir(fi); });
}

folly::Future<fuse_entry_param> EdenDispatcher::lookup(
    fuse_ino_t parent,
    PathComponentPiece namepiece) {
  return inodeMap_->lookupTreeInode(parent)
      .then([name = PathComponent(namepiece)](const TreeInodePtr& tree) {
        return tree->getOrLoadChild(name);
      })
      .then([](const InodePtr& inode) {
        return inode->getattr().then([inode](fusell::Dispatcher::Attr attr) {
          inode->incNumFuseLookups();
          return computeEntryParam(inode->getNodeId(), attr);
        });
      });
}

folly::Future<fusell::Dispatcher::Attr>
EdenDispatcher::setattr(fuse_ino_t ino, const struct stat& attr, int toSet) {
  return inodeMap_->lookupInode(ino).then([attr, toSet](const InodePtr& inode) {
    return inode->setattr(attr, toSet);
  });
}

folly::Future<folly::Unit> EdenDispatcher::forget(
    fuse_ino_t ino,
    unsigned long nlookup) {
  inodeMap_->decNumFuseLookups(ino);
  return Unit{};
}

folly::Future<std::shared_ptr<fusell::FileHandle>> EdenDispatcher::open(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  return inodeMap_->lookupFileInode(ino).then(
      [fi](const FileInodePtr& inode) { return inode->open(fi); });
}

folly::Future<fusell::Dispatcher::Create> EdenDispatcher::create(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    int flags) {
  return inodeMap_->lookupTreeInode(parent)
      .then([ childName = PathComponent{name}, mode, flags ](
          const TreeInodePtr& parentInode) {
        return parentInode->create(childName, mode, flags);
      })
      .then([=](TreeInode::CreateResult created) {
        fusell::Dispatcher::Create result;
        result.entry =
            computeEntryParam(created.inode->getNodeId(), created.attr);
        result.fh = std::move(created.file);
        return result;
      });
}

folly::Future<std::string> EdenDispatcher::readlink(fuse_ino_t ino) {
  return inodeMap_->lookupFileInode(ino).then(
      [](const FileInodePtr& inode) { return inode->readlink(); });
}

folly::Future<fuse_entry_param> EdenDispatcher::mknod(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev) {
  // We intentionally do not support device nodes.
  // The mknod(3) man page indicates that EPERM should be thrown if the
  // filesystem does not support the type of node requested.
  folly::throwSystemErrorExplicit(
      EPERM, "device node creation not supported in eden mount points");
}

folly::Future<fuse_entry_param>
EdenDispatcher::mkdir(fuse_ino_t parent, PathComponentPiece name, mode_t mode) {
  return inodeMap_->lookupTreeInode(parent).then(
      [ childName = PathComponent{name}, mode ](const TreeInodePtr& inode) {
        auto child = inode->mkdir(childName, mode);
        return child->getattr().then([childNumber = child->getNodeId()](
            fusell::Dispatcher::Attr attr) {
          return computeEntryParam(childNumber, attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::unlink(
    fuse_ino_t parent,
    PathComponentPiece name) {
  return inodeMap_->lookupTreeInode(parent)
      .then([childName = PathComponent{name}](const TreeInodePtr& inode) {
        inode->unlink(childName);
      });
}

folly::Future<folly::Unit> EdenDispatcher::rmdir(
    fuse_ino_t parent,
    PathComponentPiece name) {
  return inodeMap_->lookupTreeInode(parent)
      .then([childName = PathComponent{name}](const TreeInodePtr& inode) {
        return inode->rmdir(childName);
      });
}

folly::Future<fuse_entry_param> EdenDispatcher::symlink(
    fuse_ino_t parent,
    PathComponentPiece name,
    StringPiece link) {
  return inodeMap_->lookupTreeInode(parent).then(
      [ linkContents = link.str(),
        childName = PathComponent{name} ](const TreeInodePtr& inode) {
        return inode->symlink(childName, linkContents);
      });
}

folly::Future<folly::Unit> EdenDispatcher::rename(
    fuse_ino_t parent,
    PathComponentPiece namePiece,
    fuse_ino_t newParent,
    PathComponentPiece newNamePiece) {
  // Start looking up both parents
  auto parentFuture = inodeMap_->lookupTreeInode(parent);
  auto newParentFuture = inodeMap_->lookupTreeInode(newParent);
  // Do the rename once we have looked up both parents.
  return parentFuture.then([
    npFuture = std::move(newParentFuture),
    name = PathComponent{namePiece},
    newName = PathComponent{newNamePiece}
  ](const TreeInodePtr& parent) mutable {
    return npFuture.then(
        [parent, name, newName](const TreeInodePtr& newParent) {
          parent->rename(name, newParent, newName);
        });
  });
}

folly::Future<fuse_entry_param> EdenDispatcher::link(
    fuse_ino_t ino,
    fuse_ino_t /* newParent */,
    PathComponentPiece /* newName */) {
  // We intentionally do not support hard links.
  // These generally cannot be tracked in source control (git or mercurial)
  // and are not portable to non-Unix platforms.
  folly::throwSystemErrorExplicit(
      EPERM, "hard links are not supported in eden mount points");
}

Future<string> EdenDispatcher::getxattr(fuse_ino_t ino, StringPiece name) {
  return inodeMap_->lookupInode(ino).then([attrName = name.str()](
      const InodePtr& inode) { return inode->getxattr(attrName); });
}

Future<vector<string>> EdenDispatcher::listxattr(fuse_ino_t ino) {
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->listxattr(); });
}
}
}
