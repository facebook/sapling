/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "InodeDispatcher.h"

#include <dirent.h>
#include <wangle/concurrent/CPUThreadPoolExecutor.h>
#include <wangle/concurrent/GlobalExecutor.h>
#include <shared_mutex>
#include "Channel.h"
#include "DirHandle.h"
#include "FileHandle.h"
#include "Inodes.h"
#include "MountPoint.h"

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
namespace fusell {

InodeDispatcher::InodeDispatcher(MountPoint* mountPoint)
    : mountPoint_(mountPoint) {
  inodes_.reserve(FLAGS_inode_reserve);
}

InodeDispatcher::InodeDispatcher(
    MountPoint* mountPoint,
    std::shared_ptr<DirInode> rootInode)
    : InodeDispatcher(mountPoint) {
  if (rootInode) {
    setRootInode(std::move(rootInode));
  }
}

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
}

void InodeDispatcher::initConnection(fuse_conn_info& conn) {
  if (FLAGS_warm_kernel_on_startup) {
    auto walker = std::make_shared<Walker>(
        getChannel().getMountPoint()->getPath().stringPiece().str());
    walker->walk();
  }

  mountPoint_->mountStarted();
}

void InodeDispatcher::setRootInode(std::shared_ptr<DirInode> inode) {
  CHECK(!root_);
  CHECK_EQ(inode->getNodeId(), FUSE_ROOT_ID);
  root_ = std::move(inode);
  recordInode(root_);
}

std::shared_ptr<DirInode> InodeDispatcher::getRootInode() const {
  DCHECK(root_);
  return root_;
}

void InodeDispatcher::recordInode(std::shared_ptr<InodeBase> inode) {
  auto ino = inode->getNodeId();
  std::unique_lock<SharedMutex> g(lock_);
  auto ret = inodes_.emplace(ino, std::move(inode));
  DCHECK(ret.second);
}

std::shared_ptr<InodeBase> InodeDispatcher::getInode(
    fuse_ino_t ino,
    bool mustExist) const {
  std::shared_lock<SharedMutex> g(lock_);
  const auto& it = inodes_.find(ino);
  if (it == inodes_.end()) {
    if (mustExist) {
      throwSystemErrorExplicit(ENOENT);
    }
    return nullptr;
  }
  return it->second;
}

std::shared_ptr<InodeBase> InodeDispatcher::lookupInode(fuse_ino_t ino) const {
  std::shared_lock<SharedMutex> g(lock_);
  const auto& it = inodes_.find(ino);
  if (it == inodes_.end()) {
    return nullptr;
  }
  it->second->incNumLookups();
  return it->second;
}

std::shared_ptr<DirInode> InodeDispatcher::getDirInode(
    fuse_ino_t ino,
    bool mustExist) const {
  auto d = std::dynamic_pointer_cast<DirInode>(getInode(ino));
  if (!d) {
    if (mustExist) {
      throwSystemErrorExplicit(ENOTDIR);
    }
    return nullptr;
  }
  return d;
}

std::shared_ptr<FileInode> InodeDispatcher::getFileInode(
    fuse_ino_t ino,
    bool mustExist) const {
  auto f = std::dynamic_pointer_cast<FileInode>(getInode(ino));
  if (!f) {
    if (mustExist) {
      throwSystemErrorExplicit(EISDIR);
    }
    return nullptr;
  }
  return f;
}

folly::Future<Dispatcher::Attr> InodeDispatcher::getattr(fuse_ino_t ino) {
  return getInode(ino)->getattr();
}

folly::Future<std::shared_ptr<DirHandle>> InodeDispatcher::opendir(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  return getDirInode(ino)->opendir(fi);
}

fuse_entry_param InodeDispatcher::computeEntryParam(
    const Dispatcher::Attr& attr,
    std::shared_ptr<InodeNameManager::Node> node) {
  fuse_entry_param entry;
  entry.ino = node->getNodeId();
  entry.generation = node->getGeneration();
  entry.attr = attr.st;
  entry.attr_timeout = attr.timeout;
  entry.entry_timeout = attr.timeout;
  return entry;
}

folly::Future<fuse_entry_param> InodeDispatcher::lookup(
    fuse_ino_t parent,
    PathComponentPiece namepiece) {
  auto name = namepiece.copy();
  auto inode = lookupInodeBase(parent, namepiece).get();
  return inode->getattr().then([=](Dispatcher::Attr attr) {
    auto node = mountPoint_->getNameMgr()->getNodeById(inode->getNodeId());
    return computeEntryParam(attr, node);
  });
}

folly::Future<std::shared_ptr<InodeBase>> InodeDispatcher::lookupInodeBase(
    fuse_ino_t parent,
    PathComponentPiece namepiece) {
  auto dir = getDirInode(parent);

  // First, see if we already have the Inode loaded
  auto mgr = mountPoint_->getNameMgr();
  auto node = mgr->getNodeByName(parent, namepiece, false);
  std::shared_ptr<InodeBase> existing_inode;

  if (node) {
    existing_inode = lookupInode(node->getNodeId());
  }

  return (existing_inode ? makeFuture(existing_inode)
                         : dir->getChildByName(namepiece))
      .then([=](std::shared_ptr<InodeBase> inode) mutable {
        if (!inode) {
          throwSystemErrorExplicit(ENOENT);
        }
        if (!existing_inode) {
          // We just created it
          node = mgr->getNodeById(inode->getNodeId());
          recordInode(inode);
        }

        return inode;
      });
}

folly::Future<Dispatcher::Attr>
InodeDispatcher::setattr(fuse_ino_t ino, const struct stat& attr, int to_set) {
  return getInode(ino)->setattr(attr, to_set);
}

folly::Future<folly::Unit> InodeDispatcher::forget(
    fuse_ino_t ino,
    unsigned long nlookup) {
  {
    std::shared_lock<SharedMutex> g(lock_);
    const auto& it = inodes_.find(ino);
    if (it == inodes_.end()) {
      LOG(ERROR) << "FORGET " << ino << " nlookup=" << nlookup
                 << ", but we have no such inode!?";
      return Unit{};
    }
    if (it->second->decNumLookups(nlookup) != 0) {
      // No further work needed
      return Unit{};
    }
  }

  // No more refs; remove it
  {
    std::unique_lock<SharedMutex> g(lock_);
    auto it = inodes_.find(ino);

    if (it != inodes_.end() || !it->second->canForget()) {
      return Unit{};
    }

    inodes_.erase(it);
    LOG_EVERY_N(INFO, FLAGS_inode_reserve / 100)
        << "FORGET, now have " << inodes_.size() << " live inodes";
  }

  return Unit{};
}

folly::Future<std::shared_ptr<FileHandle>> InodeDispatcher::open(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  auto f = getFileInode(ino);
  return f->open(fi);
}

folly::Future<Dispatcher::Create> InodeDispatcher::create(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    int flags) {
  return getDirInode(parent)
      ->create(name, mode, flags)
      .then([=](DirInode::CreateResult created) {
        recordInode(created.inode);

        Dispatcher::Create result;
        result.entry = computeEntryParam(created.attr, created.node);
        result.fh = std::move(created.file);
        return result;
      });
}

folly::Future<std::string> InodeDispatcher::readlink(fuse_ino_t ino) {
  auto f = getFileInode(ino);
  return f->readlink();
}

folly::Future<fuse_entry_param> InodeDispatcher::mknod(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev) {
  return getDirInode(parent)->mknod(name, mode, rdev);
}

folly::Future<fuse_entry_param> InodeDispatcher::mkdir(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode) {
  return getDirInode(parent)->mkdir(name, mode);
}

folly::Future<folly::Unit> InodeDispatcher::unlink(
    fuse_ino_t parent,
    PathComponentPiece name) {
  return getDirInode(parent)->unlink(name).then(
      [ =, name = PathComponent(name) ] {
        auto mgr = mountPoint_->getNameMgr();
        mgr->unlink(parent, name);
        return Unit{};
      });
}

folly::Future<folly::Unit> InodeDispatcher::rmdir(
    fuse_ino_t parent,
    PathComponentPiece name) {
  return getDirInode(parent)->rmdir(name).then(
      [ =, name = PathComponent(name) ] {
        auto mgr = mountPoint_->getNameMgr();
        mgr->unlink(parent, name);
        return Unit{};
      });
}

folly::Future<fuse_entry_param> InodeDispatcher::symlink(
    PathComponentPiece link,
    fuse_ino_t parent,
    PathComponentPiece name) {
  return getDirInode(parent)->symlink(link, name);
}

folly::Future<folly::Unit> InodeDispatcher::rename(
    fuse_ino_t parent,
    PathComponentPiece name,
    fuse_ino_t newparent,
    PathComponentPiece newname) {
  return getDirInode(parent)
      ->rename(name, getDirInode(newparent), newname)
      .then([ =, name = name.copy(), newname = newname.copy() ] {
        auto mgr = mountPoint_->getNameMgr();
        mgr->rename(parent, name, newparent, newname);
        return Unit{};
      });
}

folly::Future<fuse_entry_param> InodeDispatcher::link(
    fuse_ino_t ino,
    fuse_ino_t newparent,
    PathComponentPiece newname) {
  return getInode(ino)->link(getDirInode(newparent), newname).then([
    =,
    name = newname.copy()
  ](fuse_entry_param && entry) {
    auto mgr = mountPoint_->getNameMgr();
    mgr->link(ino, entry.generation, newparent, name);
    return entry;
  });
}

Future<string> InodeDispatcher::getxattr(fuse_ino_t ino, StringPiece name) {
  return getInode(ino)->getxattr(name);
}

Future<vector<string>> InodeDispatcher::listxattr(fuse_ino_t ino) {
  return getInode(ino)->listxattr();
}
}
}
}
