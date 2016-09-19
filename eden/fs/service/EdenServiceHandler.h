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
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class EdenServer;
class TreeInode;

/*
 * Handler for the EdenService thrift interface
 */
class EdenServiceHandler : virtual public EdenServiceSvIf,
                           public facebook::fb303::FacebookBase2 {
 public:
  explicit EdenServiceHandler(EdenServer* server);

  facebook::fb303::cpp2::fb_status getStatus() override;

  void mount(std::unique_ptr<MountInfo> info) override;

  void unmount(std::unique_ptr<std::string> mountPoint) override;

  void listMounts(std::vector<MountInfo>& results) override;

  void checkOutRevision(
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::string> hash) override;

  void getBindMounts(
      std::vector<std::string>& out,
      std::unique_ptr<std::string> mountPoint) override;

  void getSHA1(
      std::vector<SHA1Result>& out,
      std::unique_ptr<std::string> mountPoint,
      std::unique_ptr<std::vector<std::string>> paths) override;

  void getMaterializedEntries(
      MaterializedResult& out,
      std::unique_ptr<std::string> mountPoint) override;

  void getCurrentJournalPosition(
      JournalPosition& out,
      std::unique_ptr<std::string> mountPoint) override;

  /**
   * When this Thrift handler is notified to shutdown, it notifies the
   * EdenServer to shut down, as well.
   */
  void shutdown() override;

 private:
  // Forbidden copy constructor and assignment operator
  EdenServiceHandler(EdenServiceHandler const&) = delete;
  EdenServiceHandler& operator=(EdenServiceHandler const&) = delete;

  SHA1Result getSHA1ForPath(
      const std::string& mountPoint,
      const std::string& path);

  SHA1Result getSHA1ForPathDefensively(
      const std::string& mountPoint,
      const std::string& path);

  void mountImpl(const MountInfo& info);

  void getMaterializedEntriesRecursive(
      std::map<std::string, FileInformation>& out,
      RelativePathPiece dirPath,
      TreeInode* dir);

  EdenServer* const server_;
};
}
} // facebook::eden
