/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/mononoke/CurlHttpClient.h"

#include <folly/io/IOBufQueue.h>
#include <folly/logging/xlog.h>
#include <folly/synchronization/CallOnce.h>

using folly::call_once;
using folly::once_flag;

namespace facebook {
namespace eden {

namespace {
static size_t
write_callback(char* contents, size_t size, size_t nmemb, void* out) {
  auto result = static_cast<folly::IOBufQueue*>(out);
  auto length = size * nmemb;
  result->append(contents, length);
  return length;
}
} // namespace

CurlHttpClient::CurlHttpClient(
    folly::SocketAddress address,
    AbsolutePath certificate,
    std::chrono::milliseconds timeout)
    : address_(std::move(address)),
      certificate_(std::move(certificate)),
      timeout_(timeout) {
  handle_ = buildRequest();
}

/// Makes an HTTP GET request to the given path.
std::unique_ptr<folly::IOBuf> CurlHttpClient::get(const std::string& path) {
  auto buffer = folly::IOBufQueue{};

  if (curl_easy_setopt(handle_.get(), CURLOPT_WRITEDATA, &buffer) != CURLE_OK) {
    throw std::runtime_error("curl failed to set CURLOPT_WRITEDATA");
  }

  auto url = folly::to<std::string>(
      "https://", address_.getHostStr(), ":", address_.getPort(), path);

  if (curl_easy_setopt(handle_.get(), CURLOPT_URL, url.c_str()) != CURLE_OK) {
    throw std::runtime_error(
        folly::to<std::string>("curl failed to set url: ", url));
  }

  auto ret = curl_easy_perform(handle_.get());
  if (ret != CURLE_OK) {
    throw std::runtime_error(folly::to<std::string>(
        "curl error: while fetching ",
        path,
        " code: ",
        curl_easy_strerror(ret)));
  }

  long statusCode;

  if (curl_easy_getinfo(handle_.get(), CURLINFO_RESPONSE_CODE, &statusCode) !=
      CURLE_OK) {
    throw std::runtime_error("curl failed to get response code");
  }

  if (statusCode != 200) {
    throw std::runtime_error(folly::to<std::string>(
        "received ",
        statusCode,
        " error when fetching '",
        path,
        "' to Mononoke API Server"));
  }

  auto result = buffer.move();
  if (!result) {
    return std::make_unique<folly::IOBuf>();
  }

  // make sure the caller of this function gets the response in one piece
  result->coalesce();
  return result;
}

std::unique_ptr<CURL, CurlDeleter> CurlHttpClient::buildRequest() {
  CURL* curl = curl_easy_init();
  if (!curl) {
    throw std::runtime_error("failed to create easy handle");
  }

  auto request = std::unique_ptr<CURL, CurlDeleter>{curl};

  if (curl_easy_setopt(request.get(), CURLOPT_SSLCERT, certificate_.c_str()) !=
      CURLE_OK) {
    throw std::runtime_error(folly::to<std::string>(
        "curl failed to set client certificate: ", certificate_));
  }
  if (curl_easy_setopt(
          request.get(), CURLOPT_HTTP_VERSION, CURL_HTTP_VERSION_2TLS) !=
      CURLE_OK) {
    throw std::runtime_error("curl failed to set http version");
  }
  if (curl_easy_setopt(request.get(), CURLOPT_TIMEOUT_MS, timeout_) !=
      CURLE_OK) {
    throw std::runtime_error(folly::to<std::string>(
        "curl failed to set timeout: ", timeout_.count()));
  }

  if (curl_easy_setopt(request.get(), CURLOPT_WRITEFUNCTION, write_callback) !=
      CURLE_OK) {
    throw std::runtime_error("curl failed to set write function");
  }

  // It appears that we don't have rootcanal certificate available on Mac
  // This is insecure, need to be fixed when possible.
  if (curl_easy_setopt(request.get(), CURLOPT_SSL_VERIFYPEER, false) !=
      CURLE_OK) {
    throw std::runtime_error("curl failed to set CURLOPT_SSL_VERIFYPEER");
  }
  if (curl_easy_setopt(request.get(), CURLOPT_SSL_VERIFYHOST, false)) {
    throw std::runtime_error("curl failed to set CURLOPT_SSL_VERIFYHOST");
  }

  return request;
}
} // namespace eden
} // namespace facebook
