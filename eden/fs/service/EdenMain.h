/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>

#include "eden/fs/store/hg/MetadataImporter.h"
#include "eden/fs/telemetry/IActivityRecorder.h"

namespace facebook {
namespace eden {

class EdenConfig;
class EdenServer;
class IHiveLogger;
struct SessionInfo;

/**
 * Hooks to customize the flavor of the edenfs daemon build.
 */
class EdenMain {
 public:
  virtual ~EdenMain() = default;

  virtual std::string getEdenfsBuildName() = 0;
  virtual std::string getEdenfsVersion() = 0;
  virtual std::string getLocalHostname() = 0;
  virtual void didFollyInit() = 0;
  virtual void prepare(const EdenServer& server) = 0;
  virtual void cleanup() = 0;
  virtual MetadataImporterFactory getMetadataImporterFactory(
      std::shared_ptr<EdenConfig> edenConfig) = 0;
  virtual ActivityRecorderFactory getActivityRecorderFactory() = 0;
  virtual std::shared_ptr<IHiveLogger> getHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<EdenConfig> edenConfig) = 0;

  void runServer(const EdenServer& server);
};

/**
 * A default, open-source implementation of EdenMain.
 */
class DefaultEdenMain : public EdenMain {
 public:
  virtual std::string getEdenfsBuildName() override;
  virtual std::string getEdenfsVersion() override;
  virtual std::string getLocalHostname() override;
  virtual void didFollyInit() override;
  virtual void prepare(const EdenServer& server) override;
  virtual void cleanup() override {}
  virtual MetadataImporterFactory getMetadataImporterFactory(
      std::shared_ptr<EdenConfig> edenConfig) override;
  virtual ActivityRecorderFactory getActivityRecorderFactory() override;
  virtual std::shared_ptr<IHiveLogger> getHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<EdenConfig> edenConfig) override;
};

int runEdenMain(EdenMain&& main, int argc, char** argv);

} // namespace eden
} // namespace facebook
