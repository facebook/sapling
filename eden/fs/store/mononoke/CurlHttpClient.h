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
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <memory>
#include <optional>
#include <string>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class ServiceAddress;

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
      std::shared_ptr<ServiceAddress> service,
      AbsolutePath certificate,
      std::chrono::milliseconds timeout);

  std::unique_ptr<folly::IOBuf> get(folly::StringPiece path);

 private:
  void initGlobal();
  std::unique_ptr<CURL, CurlDeleter> buildRequest();
  std::string buildAddress(folly::StringPiece path);

  std::shared_ptr<ServiceAddress> service_;
  std::optional<folly::SocketAddress> address_;
  AbsolutePath certificate_;

  // cURL timeout for the request (see CURLOPT_TIMEOUT_MS for detail)
  const std::chrono::milliseconds timeout_;

  std::unique_ptr<CURL, CurlDeleter> handle_;
};
} // namespace eden
} // namespace facebook
