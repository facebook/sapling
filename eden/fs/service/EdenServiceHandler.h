/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "common/fb303/cpp/FacebookBase2.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

namespace facebook {
namespace eden {

class EdenServer;

/*
 * Handler for the EdenService thrift interface
 */
class EdenServiceHandler : virtual public EdenServiceSvIf,
                           public facebook::fb303::FacebookBase2 {
 public:
  explicit EdenServiceHandler(EdenServer* server);

  facebook::fb303::cpp2::fb_status getStatus() override;

  void mount(std::unique_ptr<MountInfo> info) override;

  void listMounts(std::vector<MountInfo>& results) override;

  void checkOutRevision(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> hash) override;

 private:
  // Forbidden copy constructor and assignment operator
  EdenServiceHandler(EdenServiceHandler const&) = delete;
  EdenServiceHandler& operator=(EdenServiceHandler const&) = delete;

  void mountImpl(const MountInfo& info);

  EdenServer* const server_;
};
}
} // facebook::eden
