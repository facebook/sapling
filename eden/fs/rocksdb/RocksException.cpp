/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
} // namespace eden
} // namespace facebook
