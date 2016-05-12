/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileHandleBase.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

FileHandleBase::~FileHandleBase() {}

folly::Future<FileHandleBase::Ioctl> FileHandleBase::ioctl(
    int cmd, const void* arg, folly::ByteRange inputData, size_t outputSize) {
  FUSELL_NOT_IMPL();
}

folly::Future<unsigned> FileHandleBase::poll(std::unique_ptr<PollHandle> ph) {
  FUSELL_NOT_IMPL();
}
}
}
}
