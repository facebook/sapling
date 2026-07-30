/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include <gtest/gtest.h>

#include <memory>
#include <string>
#include <variant>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

namespace facebook::eden {

class PrjfsDispatcherImplTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFile("adir/file.txt", "contents");
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

} // namespace facebook::eden

#endif
