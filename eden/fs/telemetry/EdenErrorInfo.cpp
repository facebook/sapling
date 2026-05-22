/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenErrorInfo.h"
#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"

namespace facebook::eden {

EdenErrorInfoBuilder EdenErrorInfo::fuse(
    const ErrorArg& error,
    std::optional<uint64_t> inode,
    std::string mountPoint,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Fuse, error, loc}
      .withInode(inode)
      .withMountPoint(std::move(mountPoint));
}

EdenErrorInfoBuilder EdenErrorInfo::nfs(
    const ErrorArg& error,
    std::optional<uint64_t> inode,
    std::string mountPoint,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Nfs, error, loc}
      .withInode(inode)
      .withMountPoint(std::move(mountPoint));
}

EdenErrorInfoBuilder EdenErrorInfo::overlay(
    const ErrorArg& error,
    std::optional<uint64_t> inode,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Overlay, error, loc}.withInode(
      inode);
}

EdenErrorInfoBuilder EdenErrorInfo::thrift(
    const ErrorArg& error,
    std::string clientCommandName,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Thrift, error, loc}
      .withClientCommandName(std::move(clientCommandName));
}

EdenErrorInfoBuilder EdenErrorInfo::prjfs(
    const ErrorArg& error,
    std::string filePath,
    std::string mountPoint,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Prjfs, error, loc}
      .withFilePath(std::move(filePath))
      .withMountPoint(std::move(mountPoint));
}

EdenErrorInfoBuilder EdenErrorInfo::backingStore(
    const ErrorArg& error,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::BackingStore, error, loc};
}

EdenErrorInfoBuilder EdenErrorInfo::objectStore(
    const ErrorArg& error,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::ObjectStore, error, loc};
}

EdenErrorInfoBuilder EdenErrorInfo::takeover(
    const ErrorArg& error,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Takeover, error, loc};
}

EdenErrorInfoBuilder EdenErrorInfo::privhelper(
    const ErrorArg& error,
    SourceInfo loc) {
  return EdenErrorInfoBuilder{EdenComponent::Privhelper, error, loc};
}

} // namespace facebook::eden
