/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <map>
#include <string>

#include "eden/fs/service/EdenServer.h"
#include "eden/fs/telemetry/IActivityRecorder.h"

namespace facebook::eden {

class EdenConfig;
class IHiveLogger;
struct SessionInfo;

/**
 * Allows EdenMain subclasses to register BackingStores.
 */
class DefaultBackingStoreFactory : public BackingStoreFactory {
 public:
  using Factory =
      std::function<std::shared_ptr<BackingStore>(const CreateParams&)>;

  std::shared_ptr<BackingStore> createBackingStore(
      folly::StringPiece type,
      const CreateParams& params) override;

  void registerFactory(folly::StringPiece type, Factory factory);

 private:
  std::map<folly::StringPiece, Factory> registered_;
};

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
  virtual ActivityRecorderFactory getActivityRecorderFactory() = 0;
  virtual std::shared_ptr<IHiveLogger> getHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<EdenConfig> edenConfig) = 0;

  void runServer(const EdenServer& server);

  BackingStoreFactory* getBackingStoreFactory() {
    return &backingStoreFactory_;
  }

 protected:
  void registerStandardBackingStores();

  void registerBackingStore(
      folly::StringPiece type,
      DefaultBackingStoreFactory::Factory factory) {
    backingStoreFactory_.registerFactory(type, std::move(factory));
  }

 private:
  DefaultBackingStoreFactory backingStoreFactory_;
};

/**
 * A default, open-source implementation of EdenMain.
 */
class DefaultEdenMain : public EdenMain {
 public:
  std::string getEdenfsBuildName() override;
  std::string getEdenfsVersion() override;
  std::string getLocalHostname() override;
  void didFollyInit() override;
  void prepare(const EdenServer& server) override;
  void cleanup() override {}
  ActivityRecorderFactory getActivityRecorderFactory() override;
  std::shared_ptr<IHiveLogger> getHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<EdenConfig> edenConfig) override;
};

int runEdenMain(EdenMain&& main, int argc, char** argv);

} // namespace facebook::eden
