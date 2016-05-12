/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <dirent.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <sys/xattr.h>
#include "PassThruInodes.h"
#include "eden/fuse/DirHandle.h"
#include "eden/fuse/MountPoint.h"
#include "eden/fuse/RequestData.h"
#include "eden/utils/PathFuncs.h"

using namespace folly;

DEFINE_double(passthru_dir_attr_timeout,
              1.0,
              "how long to cache passthru dir info");

namespace facebook {
namespace eden {
namespace fusell {

namespace {
class PassThruDirHandle : public DirHandle {
  DIR *dir_;
  fuse_ino_t parent_;
  fuse_ino_t ino_;
  AbsolutePath dirname_;
  MountPoint* mount_;

 public:
  explicit PassThruDirHandle(const PassThruDirInode* inode)
      // TODO: Maybe in the future we should just store a pointer to the
      // PassThruDirInode.  However, for now I don't know if the lifetime of the
      // PassThruDirInode will exceed our own, so I'm just grabbing copies of
      // the
      // data that we need.
      : parent_(inode->getFuseParentInode()),
        ino_(inode->getFuseInode()),
        dirname_(inode->getLocalPath()),
        mount_(inode->getMountPoint()) {
    dir_ = opendir(dirname_.c_str());
    if (!dir_) {
      throwSystemError("opendir(", dirname_, ")");
    }
  }

  ~PassThruDirHandle() {
    if (dir_) {
      closedir(dir_);
    }
  }

  folly::Future<DirList> readdir(DirList&& list, off_t off) override {
    seekdir(dir_, off);
    while (true) {
      errno = 0;
      auto d = ::readdir(dir_);
      if (!d) {
        if (errno == 0) {
          break;
        }
        throwSystemError("readdir");
      }
      struct stat st;
      memset(&st, 0, sizeof(st));
      st.st_mode = DTTOIF(d->d_type);
      auto name = folly::StringPiece(d->d_name);
      if (name == ".") {
        st.st_ino = ino_;
      } else if (name == "..") {
        st.st_ino = parent_;
      } else {
        auto node =
            mount_->getNameMgr()->getNodeByName(ino_, PathComponentPiece(name));
        st.st_ino = node->getNodeId();

        // Queue up an lstat as we're likely to want to follow up
        // with a stat soon
        cachedLstat(folly::to<fbstring>(dirname_, "/", name));
      }
      if (!list.add(d->d_name, st, telldir(dir_))) {
        break;
      }
    }
    return std::move(list);
  }

  folly::Future<Dispatcher::Attr> setattr(const struct stat& attr,
                                          int to_set) override {
    FUSELL_NOT_IMPL();
  }

  folly::Future<folly::Unit> fsyncdir(bool datasync) override {
    return Unit{};
  }

  folly::Future<Dispatcher::Attr> getattr() override {
    Dispatcher::Attr attr;
    checkUnixError(fstat(dirfd(dir_), &attr.st));
    attr.st.st_ino = ino_;
    attr.timeout = FLAGS_passthru_dir_attr_timeout;
    return attr;
  }
};
}

PassThruDirInode::PassThruDirInode(
    MountPoint* mp,
    fuse_ino_t ino,
    fuse_ino_t parent)
    : DirInode(ino), mount_(mp), ino_(ino), parent_(parent) {}

PassThruDirInodeWithRoot::PassThruDirInodeWithRoot(
    MountPoint* mp,
    AbsolutePathPiece localRoot,
    fuse_ino_t ino,
    fuse_ino_t parent)
    : PassThruDirInode(mp, ino, parent), localRoot_(localRoot) {}

AbsolutePath PassThruDirInode::getLocalPath() const {
  return getLocalPassThruInodePath(mount_, ino_);
}

AbsolutePath PassThruDirInode::getLocalPassThruInodePath(
    MountPoint* mp,
    fuse_ino_t ino) {
  DCHECK(ino != FUSE_ROOT_ID)
      << "impossible root id for PassThruDirInode or PassThruFileInode";
  auto* disp = mp->getDispatcher();
  auto nodeset = mp->getNameMgr()->resolvePathAsNodes(ino);

  // Walk up the path until we find our containing PassThruDirInodeWithRoot
  // instance, then concat the names with its getLocalPath result

  // Start at my parent.  We know that nodes is always at least size 2 or
  // more because PassThruDirInode can never be the root and the root is
  // always in index == 0.
  int idx = nodeset.nodes.size() - 2;
  while (idx >= 0) {
    auto inode = disp->getDirInode(nodeset.nodes[idx]->getNodeId());
    auto rooted = std::dynamic_pointer_cast<PassThruDirInodeWithRoot>(inode);

    if (rooted) {
      // This is our local root, walk back down and build up the path
      AbsolutePath rootPath(rooted->getLocalPath());
      std::vector<PathComponentPiece> bits;

      // Walk down
      while (++idx < nodeset.nodes.size()) {
        bits.emplace_back(nodeset.nodes[idx]->getName().stringPiece());
      }

      RelativePath localPath(bits);
      return rootPath + localPath;
    }

    // Walk up
    --idx;
  }

  LOG(ERROR) << "none of the parents of a PassThruDirInode or "
                "PassThruFileInode were an instance of "
                "PassThruDirInodeWithRoot";
  throw std::runtime_error("no PassThruDirInodeWithRoot found");
}

AbsolutePath PassThruDirInodeWithRoot::getLocalPath() const {
  return localRoot_.copy();
}

folly::Future<Dispatcher::Attr> PassThruDirInode::getattr() {
  return cachedLstat(getLocalPath().value()).then([=](struct stat&& st) {
    Dispatcher::Attr attr;
    attr.st = st;
    attr.st.st_ino = ino_;
    attr.timeout = FLAGS_passthru_dir_attr_timeout;
    return attr;
  });
}

folly::Future<DirHandle*> PassThruDirInode::opendir(
    const struct fuse_file_info& fi) {
  return new PassThruDirHandle(this);
}

folly::Future<std::shared_ptr<InodeBase>> PassThruDirInode::getChildByName(
    PathComponentPiece namepiece) {
  auto full = getLocalPath() + namepiece;

  return cachedLstat(full.value()).then([
    =,
    name = namepiece.copy()
  ](Try<struct stat> st) {
    auto mgr = mount_->getNameMgr();

    if (st.hasException()) {
      st.withException<std::system_error>([&](const std::system_error& err) {
        if (err.code().category() == std::system_category() &&
            err.code().value() == ENOENT) {
          // Somebody deleted it out from under us, we need to amend our state
          auto node = mgr->getNodeByName(ino_, name, false);
          if (node) {
            // Record that it has been deleted
            mgr->unlink(ino_, name);
          }
        }
      });
      LOG(ERROR) << "lstat: " << full << " rel:" << name << ": "
                 << st.exception().what();
      st.throwIfFailed();
    }

    auto node = mgr->getNodeByName(ino_, name);
    std::shared_ptr<InodeBase> inode;

    if (S_ISDIR(st.value().st_mode)) {
      inode =
          std::make_shared<PassThruDirInode>(mount_, node->getNodeId(), ino_);
    } else {
      inode =
          std::make_shared<PassThruFileInode>(mount_, node->getNodeId(), ino_);
    }
    return inode;
  });
}

folly::Future<fuse_entry_param>
PassThruDirInode::mknod(PathComponentPiece name, mode_t mode, dev_t rdev) {
  auto full = getLocalPath() + name;
  auto res = S_ISFIFO(mode) ? ::mkfifo(full.c_str(), mode)
                            : ::mknod(full.c_str(), mode, rdev);
  checkUnixError(res, "mknod(", full, ", ", mode, ", ", rdev, ")");
  return RequestData::get().getDispatcher()->lookup(getNodeId(), name);
}

folly::Future<fuse_entry_param> PassThruDirInode::mkdir(
    PathComponentPiece name,
    mode_t mode) {
  auto full = getLocalPath() + name;
  auto res = ::mkdir(full.c_str(), mode);
  checkUnixError(res, "mkdir(", full, ", ", mode, ")");
  return RequestData::get().getDispatcher()->lookup(getNodeId(), name);
}

folly::Future<folly::Unit> PassThruDirInode::unlink(PathComponentPiece name) {
  auto full = getLocalPath() + name;
  auto res = ::unlink(full.c_str());
  checkUnixError(res, "unlink(", full, ")");
  return Unit{};
}

folly::Future<folly::Unit> PassThruDirInode::rmdir(PathComponentPiece name) {
  auto full = getLocalPath() + name;
  auto res = ::rmdir(full.c_str());
  checkUnixError(res, "rmdir(", full, ")");
  return Unit{};
}

folly::Future<fuse_entry_param> PassThruDirInode::symlink(
    PathComponentPiece link,
    PathComponentPiece name) {
  auto full = getLocalPath() + name;
  auto res = ::symlink(full.c_str(), link.value().str().c_str());
  checkUnixError(res, "symlink(", full, ", ", link, ")");
  return RequestData::get().getDispatcher()->lookup(getNodeId(), name);
}

folly::Future<folly::Unit> PassThruDirInode::rename(
    PathComponentPiece name,
    std::shared_ptr<DirInode> newparent,
    PathComponentPiece newname) {
  auto target_parent = std::dynamic_pointer_cast<PassThruDirInode>(newparent);
  if (!target_parent) {
    throwSystemErrorExplicit(EXDEV, "target dir must be a PassThruDirInode");
  }

  auto localPath = getLocalPath();
  auto source = localPath + name;
  auto dest = localPath + newname;
  checkUnixError(::rename(source.c_str(), dest.c_str()));
  return Unit{};
}

folly::Future<DirInode::CreateResult>
PassThruDirInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Attempt to create the file.
  auto full = getLocalPath() + name;
  folly::File file(full.c_str(), flags, mode);

  // Generate an inode number for this new entry.
  auto node = fusell::InodeNameManager::get()->getNodeByName(ino_, name);

  auto handle =
      std::make_unique<PassThruFileHandle>(file.release(), node->getNodeId());

  // Populate metadata.
  auto handle_ptr =
      handle.get(); // need to get this before move handle into the lambda.
  return handle_ptr->getattr().then([ =, handle = std::move(handle) ](
      fusell::Dispatcher::Attr attr) mutable {
    fusell::DirInode::CreateResult result;

    result.inode =
        std::make_shared<PassThruFileInode>(mount_, node->getNodeId(), ino_);

    result.file = std::move(handle);
    result.attr = attr;
    result.node = node;

    return result;
  });
}

folly::Future<folly::Unit> PassThruDirInode::setxattr(folly::StringPiece name,
                                      folly::StringPiece value,
                                      int flags) {
  auto localPath = getLocalPath();
  auto res = ::setxattr(localPath.c_str(),
                        name.str().c_str(),
                        value.data(),
                        value.size(),
#ifdef __APPLE__
                        0, // position
#endif
                        flags
#ifdef XATTR_NOFOLLOW
                            |
                            XATTR_NOFOLLOW
#endif
                        );
  checkUnixError(
      res, "setxattr(", localPath, ", ", name, ", ", value, ", ", flags, ")");
  return Unit{};
}

folly::Future<std::string> PassThruDirInode::getxattr(folly::StringPiece name) {
  auto localPath = getLocalPath();
  char stackbuf[512];
  size_t allocsize = sizeof(stackbuf);
  char *buf = stackbuf;

  SCOPE_EXIT {
    if (buf != stackbuf) {
      free(buf);
    }
  };

  while (true) {
    auto size = ::getxattr(localPath.c_str(),
                           name.str().c_str(),
                           buf,
                           allocsize
#ifdef __APPLE__
                           ,
                           0, // position
                           XATTR_NOFOLLOW
#endif
                           );
    if (size != -1) {
      // Success
      return std::string(buf, size);
    }

    if (errno != ERANGE) {
      throwSystemError("getxattr");
    }

    // Try again with a heap buffer until we figure out how much space we need

    // Ask the system how much space we need
    allocsize = ::getxattr(localPath.c_str(),
                           name.str().c_str(),
                           nullptr,
                           0
#ifdef __APPLE__
                           ,
                           0, // position
                           XATTR_NOFOLLOW
#endif
                           );

    if (buf == stackbuf) {
      buf = (char*)malloc(allocsize);
      if (!buf) {
        throwSystemErrorExplicit(ENOMEM);
      }
    } else {
      auto nbuf = (char*)realloc(buf, allocsize);
      if (!nbuf) {
        throwSystemErrorExplicit(ENOMEM);
      }
      buf = nbuf;
    }
  }
}

folly::Future<std::vector<std::string>> PassThruDirInode::listxattr() {
  auto localPath = getLocalPath();
  char stackbuf[512];
  size_t allocsize = sizeof(stackbuf);
  char *buf = stackbuf;

  SCOPE_EXIT {
    if (buf != stackbuf) {
      free(buf);
    }
  };

  while (true) {
    auto size = ::listxattr(localPath.c_str(),
                            buf,
                            allocsize
#ifdef XATTR_NOFOLLOW
                            ,
                            XATTR_NOFOLLOW
#endif
                            );
    if (size != -1) {
      // Success; parse out the buffer, it is a set of NUL terminated strings
      std::vector<std::string> res;
      char *end = buf + size;
      while (buf < end) {
        res.emplace_back(buf);
        buf += strlen(buf) + 1;
      }
      return res;
    }

    if (errno != ERANGE) {
      throwSystemError("listxattr");
    }

    // Try again with a heap buffer until we figure out how much space we need

    // Ask the system how much space we need
    allocsize = ::listxattr(localPath.c_str(),
                            nullptr,
                            0
#ifdef XATTR_NOFOLLOW
                            ,
                            XATTR_NOFOLLOW
#endif
                            );

    if (buf == stackbuf) {
      buf = (char*)malloc(allocsize);
      if (!buf) {
        throwSystemErrorExplicit(ENOMEM);
      }
    } else {
      auto nbuf = (char*)realloc(buf, allocsize);
      if (!nbuf) {
        throwSystemErrorExplicit(ENOMEM);
      }
      buf = nbuf;
    }
  }
}

folly::Future<folly::Unit> PassThruDirInode::removexattr(folly::StringPiece name) {
  auto localPath = getLocalPath();
  checkUnixError(::removexattr(localPath.c_str(),
                               name.str().c_str()

#ifdef XATTR_NOFOLLOW
                                   ,
                               XATTR_NOFOLLOW
#endif
                               ));
  return Unit{};
}
}
}
}
