/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>

#include "eden/fs/store/hg/MetadataImporter.h"

namespace facebook {
namespace eden {

class EdenConfig;
class EdenServer;

/**
 * Hooks to customize the flavor of the edenfs daemon build.
 */
class EdenMain {
 public:
  virtual ~EdenMain() = default;

  virtual std::string getEdenfsBuildName() = 0;
  virtual std::string getEdenfsVersion() = 0;
  virtual std::string getLocalHostname() = 0;
  virtual void prepare(const EdenServer& server) = 0;
  virtual void runServer(const EdenServer& server) = 0;
  virtual MetadataImporterFactory getMetadataImporterFactory() = 0;
};

/**
 * A default, open-source implementation of EdenMain.
 */
class DefaultEdenMain : public EdenMain {
 public:
  virtual std::string getEdenfsBuildName() override;
  virtual std::string getEdenfsVersion() override;
  virtual std::string getLocalHostname() override;
  virtual void prepare(const EdenServer& server) override;
  virtual void runServer(const EdenServer& server) override;
  virtual MetadataImporterFactory getMetadataImporterFactory() override;
};

int runEdenMain(EdenMain&& main, int argc, char** argv);

} // namespace eden
} // namespace facebook
