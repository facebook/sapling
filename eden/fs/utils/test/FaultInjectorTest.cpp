/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <eden/fs/utils/FaultInjector.h>

#include <folly/portability/GTest.h>
#include <folly/stop_watch.h>
#include <folly/test/TestUtils.h>

using namespace facebook::eden;
using namespace std::chrono_literals;

TEST(FaultInjector, matching) {
  FaultInjector fi(true);
  fi.injectError("mount", "/mnt/.*", std::invalid_argument("mnt"));
  fi.injectError("mount", "/home/user/myrepo", std::runtime_error("myrepo"), 1);
  fi.injectError("mount", ".*", std::runtime_error("catchall"));

  EXPECT_THROW_RE(
      fi.check("mount", "/home/johndoe/somerepo"),
      std::runtime_error,
      "catchall");
  // The /home/user/myrepo check only matches once, so a second call to it will
  // fall through to the catch-all pattern.
  EXPECT_THROW_RE(
      fi.check("mount", "/home/user/myrepo"), std::runtime_error, "myrepo");
  EXPECT_THROW_RE(
      fi.check("mount", "/home/user/myrepo"), std::runtime_error, "catchall");

  // Test checkAsync()
  auto future = fi.checkAsync("mount", "/mnt/test");
  EXPECT_THROW_RE(std::move(future).get(10ms), std::invalid_argument, "mnt");

  // removeFault()
  EXPECT_FALSE(fi.removeFault("mount", "notdefined"));
  EXPECT_TRUE(fi.removeFault("mount", ".*"));
  EXPECT_FALSE(fi.removeFault("mount", ".*"));
  fi.check("mount", "/a/b/c");
  fi.checkAsync("mount", "/a/b/c").get();

  // Test a key class with no errors defined
  fi.check("fetch_blob", "12345678");

  // Inject an error for the key class.
  // Test a matching value and non-matching value.
  fi.injectError("fetch_blob", "12345678", std::runtime_error("fetch_blob"));
  EXPECT_THROW_RE(
      fi.check("fetch_blob", "12345678"), std::runtime_error, "fetch_blob");
  fi.check("fetch_blob", "1234567890");
  fi.check("fetch_blob", "abc");
  fi.checkAsync("fetch_blob", "abc").get();

  // Remove the only fault defined for the fetch_blob class
  EXPECT_TRUE(fi.removeFault("fetch_blob", "12345678"));
  fi.check("fetch_blob", "12345678");
}

TEST(FaultInjector, blocking) {
  FaultInjector fi(true);
  fi.injectBlock("mount", ".*");

  auto future1 = fi.checkAsync("mount", "/x/y/z");
  EXPECT_FALSE(future1.isReady());
  auto future2 = fi.checkAsync("mount", "/a/b/c");
  EXPECT_FALSE(future1.isReady());

  // Unblock both matches
  auto countUnblocked = fi.unblock("mount", ".*");
  EXPECT_EQ(2, countUnblocked);
  ASSERT_NE(future1.isReady(), detail::kImmediateFutureAlwaysDefer);
  ASSERT_NE(future2.isReady(), detail::kImmediateFutureAlwaysDefer);
  std::move(future1).get();
  std::move(future2).get();

  future1 = fi.checkAsync("mount", "/x/y/z");
  EXPECT_FALSE(future1.isReady());
  future2 = fi.checkAsync("mount", "/a/b/c");
  EXPECT_FALSE(future1.isReady());

  // Unblock just one call with an error
  countUnblocked =
      fi.unblockWithError("mount", "/a/.*", std::runtime_error("paper jam"));
  EXPECT_EQ(1, countUnblocked);
  EXPECT_FALSE(future1.isReady());
  ASSERT_NE(future2.isReady(), detail::kImmediateFutureAlwaysDefer);
  EXPECT_THROW_RE(std::move(future2).get(), std::runtime_error, "paper jam");

  // Unblock the other call
  countUnblocked = fi.unblock("mount", "/x/y/z");
  EXPECT_EQ(1, countUnblocked);
  ASSERT_NE(future1.isReady(), detail::kImmediateFutureAlwaysDefer);
  std::move(future1).get();
  EXPECT_EQ(0, fi.unblockAll());

  future1 = fi.checkAsync("mount", "/x/y/z");
  EXPECT_FALSE(future1.isReady());
  future2 = fi.checkAsync("mount", "/a/b/c");
  EXPECT_FALSE(future1.isReady());

  countUnblocked = fi.unblockAllWithError(std::domain_error("test"));
  EXPECT_EQ(2, countUnblocked);
  ASSERT_NE(future1.isReady(), detail::kImmediateFutureAlwaysDefer);
  ASSERT_NE(future2.isReady(), detail::kImmediateFutureAlwaysDefer);
  EXPECT_THROW_RE(std::move(future1).get(), std::domain_error, "test");
  EXPECT_THROW_RE(std::move(future2).get(), std::domain_error, "test");
}

TEST(FaultInjector, delay) {
  FaultInjector fi(true);
  fi.injectDelay("mount", ".*", 20ms);
  fi.injectDelayedError("error", ".*", 20ms, std::runtime_error("slowfail"));

  folly::stop_watch<> sw;
  fi.check("mount", "/test");
  EXPECT_GE(sw.elapsed(), 20ms);

  sw.reset();
  fi.checkAsync("mount", "/mnt").get();
  EXPECT_GE(sw.elapsed(), 20ms);

  sw.reset();
  EXPECT_THROW_RE(fi.check("error", "abc"), std::runtime_error, "slowfail");
  EXPECT_GE(sw.elapsed(), 20ms);

  sw.reset();
  auto future = fi.checkAsync("error", "xyz");
  EXPECT_THROW_RE(std::move(future).get(), std::runtime_error, "slowfail");
  EXPECT_GE(sw.elapsed(), 20ms);
}

TEST(FaultInjector, noop) {
  FaultInjector fi(true);
  fi.injectNoop("mount", "/a/b/c");
  fi.injectNoop("mount", ".*", 2);
  fi.injectNoop("mount", "/x/y/z");
  fi.injectError("mount", ".*", std::runtime_error("fail"));

  // The first two calls to anything other than "/a/b/c" should trigger the
  // first no-op, which then expires.
  fi.check("mount", "/a/b/c");
  fi.check("mount", "/x/y/z");
  fi.check("mount", "/mnt/test");
  // The next call to something other than /a/b/c or /x/y/z should fail now
  EXPECT_THROW_RE(fi.check("mount", "/foo/bar"), std::runtime_error, "fail");
  // /a/b/c and /x/y/z still have no-op checks
  fi.check("mount", "/x/y/z");
  fi.check("mount", "/a/b/c");
  EXPECT_THROW_RE(fi.check("mount", "/test/test"), std::runtime_error, "fail");
}

TEST(FaultInjector, joinedKey) {
  FaultInjector fi(true);
  fi.check("my_fault", "foo", "bar");
  fi.checkAsync("my_fault", "foo", "bar").get();

  fi.injectError("my_fault", "foo, bar", std::logic_error("1 + 1 = 3"));
  EXPECT_THROW_RE(
      fi.check("my_fault", "foo", "bar"), std::logic_error, R"(1 \+ 1 = 3)");
  auto future = fi.checkAsync("my_fault", "foo", "bar");
  EXPECT_THROW_RE(std::move(future).get(), std::logic_error, R"(1 \+ 1 = 3)");
  fi.check("my_fault", "foo", "baz");
  fi.checkAsync("my_fault", "foo", "baz").get();
  fi.check("my_fault", "bar", "foo");
}
