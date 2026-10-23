/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/FieldConverter.h"

#include <gtest/gtest.h>

#ifndef _WIN32
#include <unistd.h>
#endif

using namespace facebook::eden;

TEST(FieldConverterTest, expandsEveryOccurrenceOfAVariable) {
  std::map<std::string, std::string> variables{{"USER", "testuser"}};
  auto result = FieldConverter<RelativePath>{}.fromString(
      "data/${USER}/${USER}", variables);
  EXPECT_EQ("data/testuser/testuser", result.value().asString());
}

#ifndef _WIN32
TEST(FieldConverterTest, variableValueContainingItsOwnToken) {
  std::map<std::string, std::string> variables{{"HOME", "home/${HOME}"}};
  EXPECT_DEATH(
      {
        alarm(1);
        FieldConverter<RelativePath>{}.fromString("${HOME}/.edenrc", variables);
      },
      "");
}
#endif
