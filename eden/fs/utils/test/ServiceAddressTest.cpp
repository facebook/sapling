/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ServiceAddress.h"

#include <folly/SocketAddress.h>
#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include "eden/fs/eden-config.h"

#ifdef EDEN_HAVE_SERVICEROUTER
#include <servicerouter/client/cpp2/ServiceRouter.h>
#endif

using namespace facebook::eden;

TEST(ServiceAddressTest, fromHostnameAndPort) {
  auto hostname = "::1";
  auto svc = ServiceAddress{hostname, 1234};
  auto result = svc.getSocketAddressBlocking();

  EXPECT_EQ(result->first.getAddressStr(), "::1");
  EXPECT_EQ(result->first.getPort(), 1234);
  EXPECT_EQ(result->second, "::1");
}

TEST(ServiceAddressTest, nonexistentHostname) {
  auto hostname = "this-hostname-should-never-exist";
  auto svc = ServiceAddress{hostname, 1234};
  EXPECT_THROW(svc.getSocketAddressBlocking(), std::system_error);
}

#ifdef EDEN_HAVE_SERVICEROUTER

namespace {
using namespace facebook::servicerouter;

class MockServiceCacheIf : public ServiceCacheIf {
 public:
  virtual Selection getSelection(
      const std::string& serviceName,
      const ServiceOptions& /* options */,
      const ConnConfigs& /* overrides */) override {
    Selection selection;

    if (serviceName == "mononoke-apiserver") {
      auto location = std::make_shared<HostInfoLocation>("::1", 1234);
      location->setHostname("some-hostname");

      const_cast<ServiceHosts*>(selection.hosts.get())
          ->push_back(std::make_shared<HostInfo>(
              std::make_unique<HostInfoProperties>(), std::move(location)));
    }

    return selection;
  }

  virtual void getSelectionAsync(
      const std::string& /* serviceName */,
      DebugContext&& /* dbgCtx */,
      SelectionCacheCallback&& /* callback */,
      folly::EventBase* /* eventBase */,
      ServiceOptions&& /* options */,
      ConnConfigs&& /* overrides */) override {}
};
} // namespace

TEST(ServiceAddressTest, fromSMCTier) {
  auto tier = "mononoke-apiserver";
  auto svc = ServiceAddress{tier};
  auto result = svc.addressFromSMCTier(std::make_shared<MockServiceCacheIf>());

  EXPECT_EQ(result->first.getAddressStr(), "::1");
  EXPECT_EQ(result->first.getPort(), 1234);
  EXPECT_EQ(result->second, "some-hostname");
}

TEST(ServiceAddressTest, failFromSMCTier) {
  auto tier = "nonexistent-tier";
  auto svc = ServiceAddress{tier};
  auto result = svc.addressFromSMCTier(std::make_shared<MockServiceCacheIf>());
  EXPECT_EQ(result, std::nullopt);
}

#endif
