/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/FileUtil.h>
#include <sys/time.h>
#include <sys/xattr.h>
#include "PassThruInodes.h"
#include "eden/fuse/RequestData.h"

using namespace folly;

DEFINE_double(passthru_file_attr_timeout,
              1.0,
              "how long to cache passthru file info");

namespace facebook {
namespace eden {
namespace fusell {

PassThruFileHandle::PassThruFileHandle(int fd, fuse_ino_t ino)
    : fd_(fd), ino_(ino) {}

folly::Future<folly::Unit> PassThruFileHandle::release() {
  if (fd_ != -1) {
    closeNoInt(fd_);
    fd_ = -1;
  }
  return Unit{};
}

folly::Future<BufVec> PassThruFileHandle::read(size_t size, off_t off) {
  auto buf = IOBuf::createCombined(size);
  auto res = ::read(fd_, buf->writableBuffer(), size);
  checkUnixError(res);
  buf->append(res);
  return BufVec(std::move(buf));
}

folly::Future<size_t> PassThruFileHandle::write(BufVec&& buf, off_t off) {
  auto vec = buf.getIov();
  auto xfer = ::writev(fd_, vec.data(), vec.size());
  checkUnixError(xfer);
  return xfer;
}

folly::Future<size_t> PassThruFileHandle::write(folly::StringPiece data,
                                                off_t off) {
  auto xfer = ::pwrite(fd_, data.data(), data.size(), off);
  checkUnixError(xfer);
  return xfer;
}

folly::Future<folly::Unit> PassThruFileHandle::flush(uint64_t lock_owner) {
  return Unit{};
}

folly::Future<folly::Unit> PassThruFileHandle::fsync(bool datasync) {
  auto res =
#ifndef __APPLE__
      datasync ? ::fdatasync(fd_) :
#endif
               ::fsync(fd_);
  checkUnixError(res);
  return Unit{};
}

folly::Future<Dispatcher::Attr> PassThruFileHandle::getattr() {
  Dispatcher::Attr attr;
  checkUnixError(fstat(fd_, &attr.st));
  attr.st.st_ino = ino_;
  attr.timeout = FLAGS_passthru_file_attr_timeout;
  return attr;
}

#ifdef __APPLE__
#define STAT_TIMESPEC(n) st_##n##timespec
#else
#define STAT_TIMESPEC(n) st_##n##tim
#endif

static inline struct timeval timespec_to_timeval(const struct timespec& ts) {
  struct timeval tv;
  TIMESPEC_TO_TIMEVAL(&tv, &ts);
  return tv;
}

folly::Future<Dispatcher::Attr> PassThruFileHandle::setattr(
    const struct stat& attr, int to_set) {

  struct stat existing;
  checkUnixError(::fstat(fd_, &existing));

  if (to_set & FUSE_SET_ATTR_MODE) {
    checkUnixError(::fchmod(fd_, attr.st_mode));
  }
  if (to_set & (FUSE_SET_ATTR_UID|FUSE_SET_ATTR_GID)) {
    auto uid = to_set & FUSE_SET_ATTR_UID ? attr.st_uid : existing.st_uid;
    auto gid = to_set & FUSE_SET_ATTR_GID ? attr.st_gid : existing.st_gid;
    checkUnixError(::fchown(fd_, uid, gid));
  }
  if (to_set & FUSE_SET_ATTR_SIZE) {
    checkUnixError(::ftruncate(fd_, attr.st_size));
  }
  if (to_set & (FUSE_SET_ATTR_ATIME|FUSE_SET_ATTR_MTIME)) {
    struct timeval times[2];

    times[0] = timespec_to_timeval(to_set & FUSE_SET_ATTR_ATIME
                                       ? attr.STAT_TIMESPEC(a)
                                       : existing.STAT_TIMESPEC(a));
    times[1] = timespec_to_timeval(to_set & FUSE_SET_ATTR_MTIME
                                       ? attr.STAT_TIMESPEC(m)
                                       : existing.STAT_TIMESPEC(m));

    checkUnixError(::futimes(fd_, times));
  }

  return getattr();
}

PassThruFileInode::PassThruFileInode(
    MountPoint* mp,
    fuse_ino_t ino,
    fuse_ino_t parent)
    : FileInode(ino), mount_(mp), ino_(ino), parent_(parent) {}

AbsolutePath PassThruFileInode::getLocalPath() const {
  return PassThruDirInode::getLocalPassThruInodePath(mount_, ino_);
}

folly::Future<Dispatcher::Attr> PassThruFileInode::getattr() {
  auto localPath = getLocalPath();
  return cachedLstat(localPath.value()).then([=](struct stat&& st) {
    Dispatcher::Attr attr;
    attr.st = st;
    attr.st.st_ino = ino_;
    attr.timeout = FLAGS_passthru_file_attr_timeout;
    return attr;
  });
}

folly::Future<FileHandle*> PassThruFileInode::open(
    const struct fuse_file_info& fi) {
  auto localPath = getLocalPath();
  auto fd = ::open(localPath.c_str(), fi.flags);
  checkUnixError(fd);
  return new PassThruFileHandle(fd, ino_);
}

folly::Future<std::string> PassThruFileInode::readlink() {
  struct stat st;
  auto localPath = getLocalPath();
  checkUnixError(lstat(localPath.c_str(), &st));
  std::unique_ptr<char[]> buf(new char[st.st_size + 1]);
  auto res = ::readlink(localPath.c_str(), buf.get(), st.st_size+1);
  checkUnixError(res);
  buf.get()[res] = 0;
  return buf.get();
}

folly::Future<folly::Unit> PassThruFileInode::setxattr(folly::StringPiece name,
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

folly::Future<std::string> PassThruFileInode::getxattr(folly::StringPiece name) {
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

folly::Future<std::vector<std::string>> PassThruFileInode::listxattr() {
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

folly::Future<folly::Unit> PassThruFileInode::removexattr(folly::StringPiece name) {
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

folly::Future<fuse_entry_param> PassThruFileInode::link(
    std::shared_ptr<DirInode> newparent,
    PathComponentPiece newname) {
  auto target_parent = std::dynamic_pointer_cast<PassThruDirInode>(newparent);
  if (!target_parent) {
    throwSystemErrorExplicit(EXDEV, "target dir must be a PassThruDirInode");
  }
  throwSystemErrorExplicit(EACCES,
                           "cannot create hardlinks until InodeNameManager can "
                           "deal with the ambiguity");

  auto localPath = getLocalPath();
  auto dest = target_parent->getLocalPath() + newname;
  checkUnixError(::link(localPath.c_str(), dest.c_str()));
  return RequestData::get().getDispatcher()->lookup(
      target_parent->getNodeId(), newname);
}

}
}
}
