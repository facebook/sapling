/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <curl/curl.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <memory>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

struct CurlDeleter {
  void operator()(CURL* p) const {
    curl_easy_cleanup(p);
  }
};

class CurlHttpClient {
 public:
  CurlHttpClient(
      std::string host,
      AbsolutePath certificate,
      std::chrono::milliseconds timeout,
      std::shared_ptr<folly::Executor> executor);

  folly::Future<std::unique_ptr<folly::IOBuf>> futureGet(std::string path);

 private:
  void initGlobal();

  std::unique_ptr<folly::IOBuf> get(const std::string& path);
  std::unique_ptr<CURL, CurlDeleter> buildRequest(const std::string& path);

  std::string host_;
  AbsolutePath certificate_;

  // cURL timeout for the request (see CURLOPT_TIMEOUT_MS for detail)
  const std::chrono::milliseconds timeout_;

  std::shared_ptr<folly::Executor> executor_;
};
} // namespace eden
} // namespace facebook
