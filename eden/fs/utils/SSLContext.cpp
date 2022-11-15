/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "SSLContext.h"

#include <folly/io/async/SSLContext.h>
#include <folly/io/async/SSLOptions.h>
#include <folly/logging/xlog.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
std::shared_ptr<folly::SSLContext> buildSSLContext(
    std::optional<AbsolutePath> clientCertificate) {
  auto sslContext = std::make_shared<folly::SSLContext>();
  if (clientCertificate) {
    auto path = fmt::to_string(clientCertificate.value());
    XLOG(DBG2) << "build SSLContext with client certificate: " << path;
    sslContext->loadCertificate(path.c_str(), "PEM");
    sslContext->loadPrivateKey(path.c_str(), "PEM");
  }
  folly::ssl::SSLCommonOptions::setClientOptions(*sslContext);

  return sslContext;
}
} // namespace facebook::eden
