/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/FieldConverter.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(FieldConverterTest, expandsEveryOccurrenceOfAVariable) {
  std::map<std::string, std::string> variables{{"USER", "testuser"}};
  auto result = FieldConverter<RelativePath>{}.fromString(
      "data/${USER}/${USER}", variables);
  EXPECT_EQ("data/testuser/testuser", result.value().asString());
}

TEST(FieldConverterTest, variableValueContainingItsOwnToken) {
  std::map<std::string, std::string> variables{{"HOME", "home/${HOME}"}};
  auto result =
      FieldConverter<RelativePath>{}.fromString("${HOME}/.edenrc", variables);
  EXPECT_EQ("home/${HOME}/.edenrc", result.value().asString());
}
