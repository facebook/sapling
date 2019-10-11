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

namespace facebook {
namespace eden {

class Pipe;

class Subprocess {
 public:
  Subprocess();
  Subprocess(const std::vector<std::string>& cmd);
  ~Subprocess();

  void createSubprocess(
      const std::vector<std::string>& cmd,
      const char* currentDir = nullptr,
      std::unique_ptr<Pipe> childInPipe = nullptr,
      std::unique_ptr<Pipe> childOutPipe = nullptr);

  std::unique_ptr<Pipe> childInPipe_;
  std::unique_ptr<Pipe> childOutPipe_;

 private:
  const int bufferSize_ = 4096;
};

} // namespace eden
} // namespace facebook
