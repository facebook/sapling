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
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <memory>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

struct CurlshDeleter {
  void operator()(CURL* p) const {
    curl_share_cleanup(p);
  }
};

struct CurlDeleter {
  void operator()(CURL* p) const {
    curl_easy_cleanup(p);
  }
};

class CurlHttpClient {
 public:
  CurlHttpClient(
      folly::SocketAddress address,
      AbsolutePath certificate,
      std::chrono::milliseconds timeout);

  std::unique_ptr<folly::IOBuf> get(const std::string& path);

 private:
  void initGlobal();
  std::unique_ptr<CURL, CurlDeleter> buildRequest();

  folly::SocketAddress address_;
  AbsolutePath certificate_;

  // cURL timeout for the request (see CURLOPT_TIMEOUT_MS for detail)
  const std::chrono::milliseconds timeout_;

  std::unique_ptr<CURL, CurlDeleter> handle_;
};
} // namespace eden
} // namespace facebook
