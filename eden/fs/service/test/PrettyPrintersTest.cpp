/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/PrettyPrinters.h"
#include "eden/common/utils/DirType.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(PrettyPrinters, ostream_format_conflict_type) {
  EXPECT_EQ("MODIFIED_REMOVED", fmt::to_string(ConflictType::MODIFIED_REMOVED));
}

TEST(PrettyPrinters, ostream_format_checkout_conflict) {
  CheckoutConflict conflict;
  conflict.path() = "some/test/path";
  conflict.type() = ConflictType::REMOVED_MODIFIED;
  conflict.dtype() = static_cast<Dtype>(dtype_t::Regular);

  EXPECT_EQ(
      "CheckoutConflict(type=REMOVED_MODIFIED, path=\"some/test/path\", message=\"\")",
      fmt::to_string(conflict));
}

TEST(PrettyPrinters, ostream_format_checkout_conflict_error) {
  CheckoutConflict conflict;
  conflict.path() = "some/test/path";
  conflict.type() = ConflictType::ERROR;
  conflict.message() = "Error invalidating path";

  EXPECT_EQ(
      "CheckoutConflict(type=ERROR, path=\"some/test/path\", message=\"Error invalidating path\")",
      fmt::to_string(conflict));
}

TEST(PrettyPrinters, ostream_format_scm_file_status) {
  EXPECT_EQ("REMOVED", fmt::to_string(ScmFileStatus::REMOVED));
}

TEST(PrettyPrinters, ostream_format_mount_state) {
  EXPECT_EQ("RUNNING", fmt::to_string(MountState::RUNNING));
}
