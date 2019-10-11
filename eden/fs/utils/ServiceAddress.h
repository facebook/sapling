/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <variant>

namespace folly {
class SocketAddress;
} // namespace folly

namespace facebook {
namespace servicerouter {
class ServiceCacheIf;
}

namespace eden {

// `folly::SocketAddress` represents the IP and port of the server, and the
// `std::string` is the hostname the client should use.
using SocketAddressWithHostname = std::pair<folly::SocketAddress, std::string>;

using HostPortPair = std::pair<std::string, uint16_t>;

/// This class represents a remote service that can be identified with a
/// traditional hostname and port pair as well as a smc tier name. Users that
/// only need a socket address can use this class to avoid worrying about
/// underlying details.
class ServiceAddress {
 public:
  /// Constructs a `ServiceAddress` from SMC tier name
  explicit ServiceAddress(std::string name);
  /// Constructs a `ServiceAddress` from a pair of hostname and port;
  ServiceAddress(std::string hostname, uint16_t port);

  ServiceAddress(const ServiceAddress&) = default;
  ServiceAddress& operator=(const ServiceAddress& other) = default;

  ServiceAddress(ServiceAddress&&) = default;
  ServiceAddress& operator=(ServiceAddress&& other) = default;

  /// Synchronously gets the socket address and hostname of the service this
  /// object represents.
  ///
  /// When `ServiceAddress` is `Type::Hostname`:
  ///
  /// Throws `std::invalid_argument` if the hostname string is invalid.
  /// See `folly::SocketAddress::setFromHostPort` for details.
  ///
  /// Throws `std::system_error` if the hostname is unabled to be resolved.
  ///
  /// When `ServiceAddress` is `Type::SmcTier`:
  ///
  /// Always returns `std::nullopt` when there is no ServiceRouter support.
  ///
  /// Note: this function WILL block for performing DNS and SMC resolution.
  std::optional<SocketAddressWithHostname> getSocketAddressBlocking();

  /// Test method
  std::optional<SocketAddressWithHostname> addressFromSMCTier(
      std::shared_ptr<facebook::servicerouter::ServiceCacheIf> selector);

 private:
  std::optional<SocketAddressWithHostname> addressFromHostname();
  std::optional<SocketAddressWithHostname> addressFromSMCTier();

  std::variant<HostPortPair, std::string> name_;
};
} // namespace eden
} // namespace facebook
