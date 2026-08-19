/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include <gtest/gtest.h>

#include <process.h>

#include <functional>
#include <memory>
#include <stdexcept>
#include <string>
#include <variant>

#include <folly/executors/InlineExecutor.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

namespace facebook::eden {

namespace {

// Stack reservation for the constrained-stack helper below. Following
// a/b/c/d/e/f/g/loop -> loop exhausts kMaxSymlinkChainDepth (40) symlink
// follows, and the recursive resolver nests one continuation per path
// component per follow: 40 * 8 = 320 levels. Each level costs roughly 2 KiB
// of native stack when continuations complete inline (measured on Linux x64
// opt), so the recursive implementation needs ~660 KiB and overflows this
// reservation with STATUS_STACK_OVERFLOW. An iterative resolver keeps its
// state in a heap-allocated coroutine frame and stays around 1 KiB.
constexpr unsigned int kConstrainedStackSize = 256 * 1024;

struct ConstrainedStackTask {
  std::function<bool()> fn;
  bool result = false;
};

unsigned __stdcall runConstrainedStackTask(void* arg) {
  auto* task = static_cast<ConstrainedStackTask*>(arg);
  task->result = task->fn();
  return 0;
}

// Runs `fn` to completion on a thread whose stack is reserved at
// kConstrainedStackSize, joins it, and returns the result.
bool runOnConstrainedStackThread(std::function<bool()> fn) {
  ConstrainedStackTask task{std::move(fn)};
  auto handle = reinterpret_cast<HANDLE>(_beginthreadex(
      /*security=*/nullptr,
      kConstrainedStackSize,
      &runConstrainedStackTask,
      &task,
      STACK_SIZE_PARAM_IS_A_RESERVATION,
      /*thrdaddr=*/nullptr));
  if (handle == nullptr) {
    throw std::runtime_error("failed to create constrained-stack thread");
  }
  WaitForSingleObject(handle, INFINITE);
  CloseHandle(handle);
  return task.result;
}

} // namespace

class PrjfsDispatcherImplTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFile("adir/file.txt", "contents");
    // Mirrors the production shape from S697276: a deep multi-component path
    // whose last component is a self-referential symlink.
    builder_.setSymlink("a/b/c/d/e/f/g/loop", "loop");
    builder_.setSymlink("again", ".");
    builder_.setSymlink("shallow", "adir");
    mount_.initialize(builder_);
    dispatcher_ =
        std::make_unique<PrjfsDispatcherImpl>(mount_.getEdenMount().get());
  }

  // Symlink targets reach determineTargetType with '\' separators, because the
  // callers normalize '/' to '\' before calling; mirror that here.
  std::variant<AbsolutePath, RelativePath> classify(
      RelativePath symlink,
      const std::string& target) {
    return dispatcher_->determineTargetType(std::move(symlink), target);
  }

  FakeTreeBuilder builder_;
  TestMount mount_;
  std::unique_ptr<PrjfsDispatcherImpl> dispatcher_;
};

// A symlink whose target is a bare drive-letter absolute path (C:\...) pointing
// inside the mount must resolve to the corresponding in-mount RelativePath.
TEST_F(
    PrjfsDispatcherImplTest,
    driveLetterAbsoluteInsideMountResolvesRelative) {
  auto target =
      mount_.getEdenMount()->getPath().stringWithoutUNC() + "\\adir\\file.txt";

  auto result = classify(RelativePath{"link"}, target);

  ASSERT_TRUE(std::holds_alternative<RelativePath>(result));
  EXPECT_EQ(RelativePath{"adir/file.txt"}, std::get<RelativePath>(result));
}

// The UNC form (\\?\C:\...) of an in-mount absolute target must resolve the
// same way; this already worked and must keep working.
TEST_F(PrjfsDispatcherImplTest, uncAbsoluteInsideMountResolvesRelative) {
  auto target =
      mount_.getEdenMount()->getPath().asString() + "\\adir\\file.txt";

  auto result = classify(RelativePath{"link"}, target);

  ASSERT_TRUE(std::holds_alternative<RelativePath>(result));
  EXPECT_EQ(RelativePath{"adir/file.txt"}, std::get<RelativePath>(result));
}

// An absolute target OUTSIDE the mount stays absolute (resolved by the OS).
TEST_F(PrjfsDispatcherImplTest, driveLetterAbsoluteOutsideMountStaysAbsolute) {
  auto result =
      classify(RelativePath{"link"}, "C:\\definitely\\outside\\the\\mount");

  EXPECT_TRUE(std::holds_alternative<AbsolutePath>(result));
}

// A relative target is resolved against the symlink's own directory.
TEST_F(PrjfsDispatcherImplTest, relativeTargetJoinsSymlinkDirectory) {
  auto result = classify(RelativePath{"adir/link"}, "file.txt");

  ASSERT_TRUE(std::holds_alternative<RelativePath>(result));
  EXPECT_EQ(RelativePath{"adir/file.txt"}, std::get<RelativePath>(result));
}

// The recursive resolver crashes with STATUS_STACK_OVERFLOW when the whole
// resolution runs on the constrained stack. The future must be driven through
// an inline executor: a plain .get() drives the deferred chain through a
// trampolining executor and never nests, while inline-executor completion
// nests a native frame per continuation, like the production PrjFS callback
// path that overflowed a 2 MB stack with ~2,500 frames (S697276).
TEST_F(
    PrjfsDispatcherImplTest,
    selfReferentialSymlinkOverflowsConstrainedStack) {
  EXPECT_DEATH(
      runOnConstrainedStackThread([&] {
        return dispatcher_
            ->isFinalSymlinkPathDirectory(
                RelativePath{"a/b/c/d/e/f/g/loop"},
                "loop",
                ObjectFetchContext::getNullContext())
            .semi()
            .via(&folly::InlineExecutor::instance())
            .get();
      }),
      "");
}

TEST_F(PrjfsDispatcherImplTest, shallowSymlinkStillResolves) {
  auto future = dispatcher_->isFinalSymlinkPathDirectory(
      RelativePath{"shallow"}, "adir", ObjectFetchContext::getNullContext());

  EXPECT_TRUE(std::move(future).get());
}

TEST_F(PrjfsDispatcherImplTest, repeatedSymlinkWithShrinkingSuffixResolves) {
  auto result = dispatcher_
                    ->resolveSymlinkPath(
                        RelativePath{"again/again/adir"},
                        ObjectFetchContext::getNullContext())
                    .get();

  ASSERT_TRUE(std::holds_alternative<RelativePath>(result));
  EXPECT_EQ(RelativePath{"adir"}, std::get<RelativePath>(result));
}

} // namespace facebook::eden

#endif
