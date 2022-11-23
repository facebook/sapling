/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/SpawnedProcess.h"
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <list>
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using Options = SpawnedProcess::Options;

#ifndef _WIN32
TEST(SpawnedProcess, cwd_slash) {
  Options opts;
  opts.nullStdin();
  opts.pipeStdout();
  opts.chdir(kRootAbsPath);
  SpawnedProcess proc({"pwd"}, std::move(opts));

  auto outputs = proc.communicate();
  proc.wait();

  EXPECT_EQ("/\n", outputs.first);
}

TEST(SpawnedProcess, cwd_inherit) {
  Options opts;
  opts.nullStdin();
  opts.pipeStdout();
  SpawnedProcess proc({"pwd"}, std::move(opts));

  auto outputs = proc.communicate();
  proc.wait();

  auto stdout = outputs.first;

  EXPECT_FALSE(stdout.empty());
  EXPECT_EQ('\n', stdout[stdout.size() - 1]);
  stdout = stdout.substr(0, stdout.size() - 1);

  char cwd[1024];
  getcwd(cwd, sizeof(cwd) - 1);

  EXPECT_EQ(realpath(cwd), realpath(stdout));
}
#endif

TEST(SpawnedProcess, pipe) {
  Options opts;
  opts.nullStdin();
  opts.pipeStdout();
  SpawnedProcess echo(
      {
#ifndef _WIN32
          "echo",
#else
          "powershell",
          "-Command",
          "echo",
#endif
          "hello"},
      std::move(opts));

  auto outputs = echo.communicate();
  echo.wait();

  folly::StringPiece line(outputs.first);
  EXPECT_EQ(line.subpiece(0, 5), "hello");
}

void test_pipe_input(bool threaded) {
#ifndef _WIN32
  Options opts;
  opts.pipeStdout();
  opts.pipeStdin();
  SpawnedProcess cat({"cat", "-"}, std::move(opts));

  std::vector<std::string> expected{"one", "two", "three"};
  std::list<std::string> lines{"one\n", "two\n", "three\n"};

  auto writable = [&lines](FileDescriptor& fd) {
    if (lines.empty()) {
      return true;
    }
    auto str = lines.front();
    if (write(fd.fd(), str.data(), str.size()) == -1) {
      throw std::runtime_error("write to child failed");
    }
    lines.pop_front();
    return false;
  };

  auto outputs =
      threaded ? cat.threadedCommunicate(writable) : cat.communicate(writable);
  cat.wait();

  std::vector<std::string> resultLines;
  folly::split('\n', outputs.first, resultLines, /*ignoreEmpty=*/true);
  EXPECT_EQ(resultLines.size(), 3);
  EXPECT_EQ(resultLines, expected);
#else
  (void)threaded;
#endif
}

TEST(SpawnedProcess, stresstest_pipe_output) {
  bool okay = true;
#ifndef _WIN32
  for (int i = 0; i < 3000; ++i) {
    Options opts;
    opts.pipeStdout();
    opts.nullStdin();
    SpawnedProcess proc({"head", "-n20", "/dev/urandom"}, std::move(opts));
    auto outputs = proc.communicate();
    folly::StringPiece out(outputs.first);
    proc.wait();
    if (out.empty() || out[out.size() - 1] != '\n') {
      okay = false;
      break;
    }
  }
#endif
  EXPECT_TRUE(okay);
}

TEST(SpawnedProcess, inputThreaded) {
  test_pipe_input(true);
}

TEST(SpawnedProcess, inputNotThreaded) {
  test_pipe_input(false);
}

TEST(SpawnedProcess, shellQuoting) {
  std::vector<std::string> args;
  if (folly::kIsWindows) {
    args.emplace_back("powershell");
    args.emplace_back("-Command");
  } else {
    args.emplace_back("/bin/sh");
    args.emplace_back("-c");
  }

  args.emplace_back("echo \"This is a test\"");

  Options opts;
  opts.nullStdin();
  opts.pipeStdout();
  SpawnedProcess proc(args, std::move(opts));
  auto outputs = proc.communicate();

  auto status = proc.wait();
  EXPECT_EQ(status.exitStatus(), 0);

  folly::StringPiece line(outputs.first);
  EXPECT_EQ(line.subpiece(0, 14), "This is a test");
}
