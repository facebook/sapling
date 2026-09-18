/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/MinVersionGate.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(MinVersionGateTest, parseEdenVersion) {
  EXPECT_EQ(
      parseEdenVersion("20260915-081814"), (EdenVersion{20260915, 81814}));
  EXPECT_EQ(parseEdenVersion("20260915"), (EdenVersion{20260915, 0}));
  EXPECT_EQ(parseEdenVersion("20260915-"), (EdenVersion{20260915, 0}));
  EXPECT_EQ(
      parseEdenVersion("20260915-081814.1"), (EdenVersion{20260915, 81814}));
  EXPECT_EQ(parseEdenVersion("-"), std::nullopt);
  EXPECT_EQ(parseEdenVersion(""), std::nullopt);
  EXPECT_EQ(parseEdenVersion("2026-09-15"), std::nullopt);
  EXPECT_EQ(parseEdenVersion("2026091"), std::nullopt);
}

TEST(MinVersionGateTest, versionOrdering) {
  EXPECT_LT((EdenVersion{20260915, 81814}), (EdenVersion{20260916, 0}));
  EXPECT_LT((EdenVersion{20260915, 0}), (EdenVersion{20260915, 1}));
  EXPECT_FALSE((EdenVersion{20260915, 1}) < (EdenVersion{20260915, 1}));
  EXPECT_EQ((EdenVersion{20260915, 1}), (EdenVersion{20260915, 1}));
}

TEST(MinVersionGateTest, parseMinVersionGatedKey) {
  auto gated = parseMinVersionGatedKey("knob@min-version=20260901");
  ASSERT_TRUE(gated.has_value());
  EXPECT_EQ(gated->name, "knob");
  EXPECT_EQ(gated->minVersion, "20260901");

  EXPECT_FALSE(parseMinVersionGatedKey("knob").has_value());
  EXPECT_FALSE(parseMinVersionGatedKey("knob@min-version").has_value());

  // A gate with nothing after it is still a gate; the caller rejects it when
  // the version fails to parse, rather than applying the entry ungated.
  auto empty = parseMinVersionGatedKey("knob@min-version=");
  ASSERT_TRUE(empty.has_value());
  EXPECT_EQ(empty->name, "knob");
  EXPECT_EQ(empty->minVersion, "");
}

TEST(MinVersionGateTest, buildVersionIsDevOrParseable) {
  // Whatever the build injected, it must be a dev build or a real release.
  // A parse failure is reported as version 0 so gates fail closed.
  auto version = getBuildEdenVersion();
  EXPECT_TRUE(!version.has_value() || version->date != 0);
}
