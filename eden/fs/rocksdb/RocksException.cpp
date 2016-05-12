/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "RocksException.h"

namespace facebook {
namespace eden {

RocksException::RocksException(
    const rocksdb::Status& status,
    const std::string& msg)
    : status_(status), msg_(msg) {
  fullMsg_ = folly::to<std::string>(msg, "(Status: ", status_.ToString(), ")");
}

const char* RocksException::what() const noexcept {
  return fullMsg_.c_str();
}

const rocksdb::Status& RocksException::getStatus() const {
  return status_;
}

const std::string& RocksException::getMsg() const {
  return msg_;
}
}
}
