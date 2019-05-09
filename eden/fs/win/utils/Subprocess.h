/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
      std::unique_ptr<Pipe> childInPipe = nullptr,
      std::unique_ptr<Pipe> childOutPipe = nullptr);

  std::unique_ptr<Pipe> childInPipe_;
  std::unique_ptr<Pipe> childOutPipe_;

 private:
  const int bufferSize_ = 4096;
};

} // namespace eden
} // namespace facebook
