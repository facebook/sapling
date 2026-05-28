/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"
#include <gtest/gtest.h>
#include <cerrno>
#include <stdexcept>
#include <system_error>

#include <folly/CPortability.h>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/fs/telemetry/DaemonError.h"

#include "eden/fs/telemetry/ErrorArg.h"
#include "eden/fs/telemetry/ThrowTraceCapture.h"

using namespace facebook::eden;

namespace {
[[noreturn]] FOLLY_NOINLINE void throwRuntimeError() {
  throw std::runtime_error("fuse read failed");
}
} // namespace

TEST(EdenErrorInfoTest, InitializeFuseEdenErrorInfoWithException) {
  try {
    throwRuntimeError();
  } catch (const std::exception& ex) {
    auto info = EdenErrorInfo::fuse(ex, 42, "/mnt/repo").create();

    EXPECT_EQ(info.component, EdenComponent::Fuse);
    EXPECT_EQ(info.errorMessage, "fuse read failed");
    EXPECT_EQ(info.inode.value(), 42);
    EXPECT_EQ(info.mountPoint.value(), "/mnt/repo");
    EXPECT_FALSE(info.errorCode.has_value());
    EXPECT_NE(
        info.exceptionType.value().find("runtime_error"), std::string::npos);
    ASSERT_TRUE(info.stackTrace.has_value());
    EXPECT_NE(info.stackTrace->find("Stack trace:"), std::string::npos)
        << "Stack trace should contain raw trace section, got: "
        << *info.stackTrace;
#ifndef _WIN32
    // Windows without PDB debug info can't resolve file paths or function names
    EXPECT_NE(
        info.stackTrace->find("EdenErrorInfoBuilderTest.cpp"),
        std::string::npos)
        << "Stack trace should contain source file, got: " << *info.stackTrace;
    EXPECT_NE(info.stackTrace->find("throwRuntimeError"), std::string::npos)
        << "Stack trace should contain the throwing function, got: "
        << *info.stackTrace;
#endif
  }
}

TEST(EdenErrorInfoTest, InitializeFuseEdenErrorInfoWithStringMessage) {
  auto info =
      EdenErrorInfo::fuse("inode failed to load", 99, "/mnt/repo").create();

  EXPECT_EQ(info.component, EdenComponent::Fuse);
  EXPECT_EQ(info.errorMessage, "inode failed to load");
  EXPECT_EQ(info.inode.value(), 99);
  EXPECT_FALSE(info.errorCode.has_value());
  EXPECT_FALSE(info.errorName.has_value());
  EXPECT_FALSE(info.exceptionType.has_value());
  ASSERT_TRUE(info.stackTrace.has_value());
  EXPECT_NE(
      info.stackTrace->find("EdenErrorInfoBuilderTest.cpp"), std::string::npos)
      << "Stack trace should contain source file, got: " << *info.stackTrace;
}

TEST(EdenErrorInfoTest, FuseErrorInfoOverridesErrorCodeAndName) {
  std::runtime_error ex("request timed out");
  auto info = EdenErrorInfo::fuse(ex, 123, "/mnt/repo")
                  .withErrorCode(ETIMEDOUT)
                  .withErrorName("ETIMEDOUT")
                  .create();

  EXPECT_EQ(info.component, EdenComponent::Fuse);
  EXPECT_EQ(info.errorMessage, "request timed out");
  EXPECT_EQ(info.inode.value(), 123);
  EXPECT_EQ(info.errorCode.value(), ETIMEDOUT);
  EXPECT_EQ(info.errorName.value(), "ETIMEDOUT");
  EXPECT_NE(
      info.exceptionType.value().find("runtime_error"), std::string::npos);
}

TEST(EdenErrorInfoTest, InitializeThriftEdenErrorInfoWithSystemError) {
  std::system_error ex(
      std::make_error_code(std::errc::permission_denied), "access denied");
  auto info = EdenErrorInfo::thrift(ex, "hg status").create();

  EXPECT_EQ(info.component, EdenComponent::Thrift);
  EXPECT_EQ(info.clientCommandName.value(), "hg status");
  EXPECT_TRUE(info.errorCode.has_value());
  EXPECT_TRUE(info.errorName.has_value());
  EXPECT_NE(info.exceptionType.value().find("system_error"), std::string::npos);
}

TEST(EdenErrorInfoTest, ClientCommandNameSerializedIntoExtrasJsonColumn) {
  std::runtime_error ex("test error");

  // Verify clientCommandName is serialized into extras JSON column
  auto event = EdenErrorInfo::thrift(ex, "mount").createEvent();
  DynamicEvent de;
  event.populate(de);
  auto it = de.getStringMap().find("extras");
  ASSERT_NE(it, de.getStringMap().end()) << "Should have extras column";
  EXPECT_NE(it->second.find("client_command_name"), std::string::npos);
  EXPECT_NE(it->second.find("mount"), std::string::npos);
}

TEST(EdenErrorInfoTest, SymbolizationIsDeferredUntilCreate) {
  try {
    throw std::runtime_error("deferred test");
  } catch (const std::exception& ex) {
    // ErrorArg should NOT consume the trace — just record that one exists.
    ErrorArg error(ex);
    EXPECT_TRUE(error.hasCapturedTrace);

    // Trace should still be available — ErrorArg only sets a flag,
    // it doesn't call getThrowSiteStackTrace().
    auto trace = getThrowSiteStackTrace();
    ASSERT_TRUE(trace.has_value())
        << "Trace should still be in thread-local storage after ErrorArg";

    // After getThrowSiteStackTrace() consumed the trace, create()
    // should still work but without a throw-site trace section.
  }
}
