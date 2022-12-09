/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ConfigVariables.h"
#include "eden/fs/utils/UserInfo.h"

namespace facebook::eden {

namespace {
const char* const kEnvSubst[] = {
    "THRIFT_TLS_CL_CERT_PATH",
};
}

ConfigVariables getUserConfigVariables(const UserInfo& userInfo) {
  ConfigVariables rv;
  rv.emplace("HOME", userInfo.getHomeDirectory().c_str());
  rv.emplace("USER", userInfo.getUsername());
  rv.emplace("USER_ID", std::to_string(userInfo.getUid()));

  for (const char* name : kEnvSubst) {
    if (const char* value = ::getenv(name)) {
      rv.emplace(name, value);
    }
  }

  return rv;
}

} // namespace facebook::eden
