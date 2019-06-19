/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/fuse/FileHandleBase.h"

using namespace folly;

namespace facebook {
namespace eden {

FileHandleBase::~FileHandleBase() {}

folly::Future<FileHandleBase::Ioctl> FileHandleBase::ioctl(
    int /*cmd*/,
    const void* /*arg*/,
    folly::ByteRange /*inputData*/,
    size_t /*outputSize*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<unsigned> FileHandleBase::poll(
    std::unique_ptr<PollHandle> /*ph*/) {
  FUSELL_NOT_IMPL();
}

} // namespace eden
} // namespace facebook
