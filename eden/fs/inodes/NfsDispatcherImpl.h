/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

  folly::Future<struct stat> getattr(
      InodeNumber ino,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::SetattrRes> setattr(
      InodeNumber ino,
      DesiredMetadata desired,
      ObjectFetchContext& context) override;

  folly::Future<InodeNumber> getParent(
      InodeNumber ino,
      ObjectFetchContext& context) override;

  folly::Future<std::tuple<InodeNumber, struct stat>> lookup(
      InodeNumber dir,
      PathComponent name,
      ObjectFetchContext& context) override;

  folly::Future<std::string> readlink(
      InodeNumber ino,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::ReadRes> read(
      InodeNumber ino,
      size_t size,
      off_t offset,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::WriteRes> write(
      InodeNumber ino,
      std::unique_ptr<folly::IOBuf> data,
      off_t offset,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::CreateRes> create(
      InodeNumber ino,
      PathComponent name,
      mode_t mode,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::MkdirRes> mkdir(
      InodeNumber ino,
      PathComponent name,
      mode_t mode,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::SymlinkRes> symlink(
      InodeNumber dir,
      PathComponent name,
      std::string data,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::UnlinkRes> unlink(
      InodeNumber dir,
      PathComponent name,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::RenameRes> rename(
      InodeNumber fromIno,
      PathComponent fromName,
      InodeNumber toIno,
      PathComponent toName,
      ObjectFetchContext& context) override;

  folly::Future<NfsDispatcher::ReaddirRes> readdir(
      InodeNumber dir,
      off_t offset,
      uint32_t count,
      ObjectFetchContext& context) override;

  folly::Future<struct statfs> statfs(
      InodeNumber ino,
      ObjectFetchContext& context) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;
  InodeMap* const inodeMap_;
};
} // namespace facebook::eden

#endif
