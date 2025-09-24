/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#pragma once

namespace facebook {
namespace network {

constexpr std::string_view DOMAIN_SUFFIX = ".facebook.com";

inline std::string getLocalHost(bool stripFbDomain) {
    std::array<char, HOST_NAME_MAX + 1> buf{};

    if (::gethostname(buf.data(), buf.size()) != 0) {
        throw std::system_error(errno, std::generic_category(), "gethostname failed");
    }

    std::string hostname(buf.data());

    if (stripFbDomain && 
        hostname.size() >= DOMAIN_SUFFIX.size() &&
        hostname.compare(hostname.size() - DOMAIN_SUFFIX.size(),
                         DOMAIN_SUFFIX.size(),
                         DOMAIN_SUFFIX) == 0)
    {
        hostname.resize(hostname.size() - DOMAIN_SUFFIX.size());
    }

    return hostname;
}

} // namespace network
} // namespace facebook
