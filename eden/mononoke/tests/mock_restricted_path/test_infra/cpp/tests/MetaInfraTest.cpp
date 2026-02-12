// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <gtest/gtest.h>

#include "configerator/distribution/api/api.h"
#include "gatekeeper/GK.h"
#include "gatekeeper/ScopedGKFake.h"
#include "justknobs/JustKnobs.h" // @manual=fbcode//justknobs:justknobs

// Define the gatekeeper we want to test
DEFINE_GATEKEEPER(testinfra_playground_always_pass);

TEST(MetaInfraTest, ReadFromConfigerator) {
  facebook::configerator::ConfigeratorApi api;
  std::string contents;

  bool success = api.getConfig(
      "testinfra/testpilot/testpilot.health_check", &contents, 5000);

  EXPECT_TRUE(success)
      << "Failed to fetch configerator config within 5 seconds";
  EXPECT_FALSE(contents.empty()) << "Config contents should not be empty";
}

TEST(MetaInfraTest, ReadFromGatekeeper) {
  fbid_t testUserId = 0;
  bool result = GATEKEEPER(testinfra_playground_always_pass).check(testUserId);

  EXPECT_TRUE(result);
}

TEST(MetaInfraTest, ReadFromMockedGatekeeper) {
  facebook::gatekeeper::ScopedGKFake fakeGK;
  fakeGK.setResult(GATEKEEPER(testinfra_playground_always_pass), true);

  fbid_t testUserId = 0;
  bool result = GATEKEEPER(testinfra_playground_always_pass).check(testUserId);
  EXPECT_TRUE(result);

  fakeGK.setResult(GATEKEEPER(testinfra_playground_always_pass), false);
  result = GATEKEEPER(testinfra_playground_always_pass).check(testUserId);
  EXPECT_FALSE(result);
}

TEST(MetaInfraTest, ReadFromJustKnobs) {
  bool knobValue = facebook::jk::eval<
      "testinfra/cpp_playground_always_pass:this_always_pass">();
  EXPECT_TRUE(knobValue);
}
