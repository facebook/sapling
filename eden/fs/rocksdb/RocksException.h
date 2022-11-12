/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/ranges.h>
#include <folly/Likely.h>
#include <rocksdb/db.h>

namespace facebook::eden {

class RocksException : public std::exception {
 public:
  RocksException(const rocksdb::Status& status, const std::string& msg);

  template <typename... Args>
  static RocksException build(
      const rocksdb::Status& status,
      const Args&... args) {
    return RocksException(
        status,
        fmt::to_string(
            fmt::join(std::make_tuple<const Args&...>(args...), "")));
  }

  template <typename... Args>
  static void check(const rocksdb::Status& status, const Args&... args) {
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
} // namespace facebook::eden
