/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/String.h>
#include <rocksdb/db.h>

namespace facebook {
namespace eden {

class RocksException : public std::exception {
 public:
  RocksException(const rocksdb::Status& status, const std::string& msg);

  template <typename... Args>
  static RocksException build(const rocksdb::Status& status, Args&&... args) {
    return RocksException(
        status, folly::to<std::string>(std::forward<Args>(args)...));
  }

  template <typename... Args>
  static void check(const rocksdb::Status& status, Args&&... args) {
    if (UNLIKELY(!status.ok())) {
      throw build(status, std::forward<Args>(args)...);
    }
  }

  const char* what() const noexcept override;
  const rocksdb::Status& getStatus() const;
  const std::string& getMsg() const;

 private:
  rocksdb::Status status_;
  std::string msg_;
  std::string fullMsg_;
};
}
}
