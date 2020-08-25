/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <string>
#include <vector>
#include <optional>

#include "eden/fs/win/utils/Pipe.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class Subprocess {
 public:
  Subprocess() = default;

  explicit Subprocess(const std::vector<std::string>& cmd) {
    createSubprocess(cmd, std::make_unique<Pipe>(), std::make_unique<Pipe>());
  }

  ~Subprocess() = default;

  void createSubprocess(
      const std::vector<std::string>& cmd,
      std::unique_ptr<Pipe> childInPipe,
      std::unique_ptr<Pipe> childOutPipe,
      std::optional<AbsolutePathPiece> currentDir = std::nullopt);

  std::unique_ptr<Pipe> childInPipe_;
  std::unique_ptr<Pipe> childOutPipe_;

 private:
  const int bufferSize_ = 4096;
};

} // namespace eden
} // namespace facebook
