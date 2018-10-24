/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "SSLContext.h"

#include <folly/io/async/SSLContext.h>
#include <folly/io/async/SSLOptions.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <glog/logging.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {
std::shared_ptr<folly::SSLContext> buildSSLContext(
    std::optional<AbsolutePath> clientCertificate) {
  auto sslContext = std::make_shared<folly::SSLContext>();
  if (clientCertificate) {
    auto path = folly::to<std::string>(clientCertificate.value());
    XLOG(DBG2) << "build SSLContext with client certificate: " << path;
    sslContext->loadCertificate(path.c_str(), "PEM");
    sslContext->loadPrivateKey(path.c_str(), "PEM");
  }
  folly::ssl::SSLCommonOptions::setClientOptions(*sslContext);

  return sslContext;
}
} // namespace eden
} // namespace facebook
