/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include "eden/fs/nfs/NfsDispatcher.h"

namespace facebook::eden {
class EdenMount;
class InodeMap;

class NfsDispatcherImpl : public NfsDispatcher {
 public:
  explicit NfsDispatcherImpl(EdenMount* mount);

  ImmediateFuture<struct stat> getattr(
      InodeNumber ino,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::SetattrRes> setattr(
      InodeNumber ino,
      DesiredMetadata desired,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<InodeNumber> getParent(
      InodeNumber ino,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<std::tuple<InodeNumber, struct stat>> lookup(
      InodeNumber dir,
      PathComponent name,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<std::string> readlink(
      InodeNumber ino,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::ReadRes> read(
      InodeNumber ino,
      size_t size,
      off_t offset,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::WriteRes> write(
      InodeNumber ino,
      std::unique_ptr<folly::IOBuf> data,
      off_t offset,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::CreateRes> create(
      InodeNumber ino,
      PathComponent name,
      mode_t mode,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::MkdirRes> mkdir(
      InodeNumber ino,
      PathComponent name,
      mode_t mode,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::SymlinkRes> symlink(
      InodeNumber dir,
      PathComponent name,
      std::string data,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::MknodRes> mknod(
      InodeNumber dir,
      PathComponent name,
      mode_t mode,
      dev_t rdev,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::UnlinkRes> unlink(
      InodeNumber dir,
      PathComponent name,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::RmdirRes> rmdir(
      InodeNumber dir,
      PathComponent name,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::RenameRes> rename(
      InodeNumber fromIno,
      PathComponent fromName,
      InodeNumber toIno,
      PathComponent toName,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::ReaddirRes> readdir(
      InodeNumber dir,
      off_t offset,
      uint32_t count,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<NfsDispatcher::ReaddirRes> readdirplus(
      InodeNumber dir,
      off_t offset,
      uint32_t count,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<struct statfs> statfs(
      InodeNumber ino,
      const ObjectFetchContextPtr& context) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;
  InodeMap* const inodeMap_;
};
} // namespace facebook::eden

#endif
