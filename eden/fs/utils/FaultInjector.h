/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <boost/regex.hpp>
#include <fmt/ranges.h>
#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <chrono>
#include <optional>
#include <string_view>
#include <variant>

#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

/**
 * A helper class for injecting artificial faults into the normal program flow.
 *
 * This allows external test code to inject delay or failures into specific
 * locations in the program.
 *
 * To use this class, add calls to FaultInjector::check() in your code anywhere
 * that you would like to be able to inject faults during testing.  During
 * normal production use these calls do nothing, and immediately return.
 * However, during tests this allows faults to be injected, causing any call to
 * FaultInjector::check() to potentially throw an exception, trigger a delay, or
 * wait until it is explicitly unblocked.  This allows exercising error handling
 * code that is otherwise difficult to trigger reliably.  This also allows
 * forcing specific ordering of events, in order to ensure that you can test
 * specific code paths.
 */
class FaultInjector {
 public:
  /**
   * Create a new FaultInjector.
   *
   * If `enabled` is false, all fault injector checks become no-ops with minimal
   * runtime overhead.  If `enabled` is true then fault injector checks are
   * evaluated, allowing exceptions or delays to be injected into the code at
   * any check.
   *
   * The normal expected use is for most programs to have a single FaultInjector
   * object, with the `enabled` setting controlled via a command line flag or
   * some other configuration read at program start-up.  During normal
   * production use `enabled` is false, allowing all fault checks to be quickly
   * skipped with minimal overhead.  During unit tests and integration tests the
   * `enabled` flag can be turned on, allowing faults to be injected in the code
   * during testing.
   */
  explicit FaultInjector(bool enabled);
  ~FaultInjector();

  /**
   * Check for an injected fault with the specified key.
   *
   * If fault injection is disabled or if there is no matching fault for this
   * (keyClass, keyValue) tuple, then this function returns immediately without
   * doing anything.
   *
   * However, if fault injection is enabled and a fault has been injected
   * matching the arguments this method may throw an exception or block for some
   * amount of time before returning (or throwing).
   *
   * Faults are identified by a (class, value) tuple.  In practice, the class
   * name is usually a fixed string literal that identifies the type of fault or
   * the location in the code where the fault is being checked.  The value
   * string may contain some additional runtime-specified value to filter the
   * fault to only trigger when this code path is hit with specific arguments.
   */
  void check(std::string_view keyClass, std::string_view keyValue) {
    if (UNLIKELY(enabled_)) {
      return checkImpl(keyClass, keyValue);
    }
  }

  /**
   * Check for an injected fault with the specified key.
   *
   * This is an async-aware implementation of check() that returns a SemiFuture.
   * This can also be used in coroutine contexts, since SemiFuture objects can
   * be co_await'ed.
   *
   * If fault injection is disabled or there is no matching fault, this method
   * will return a SemiFuture that is immediately ready.  However, if there is a
   * matching fault that would block execution this method immediately returns a
   * SemiFuture that will not be ready until the fault is complete.
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> checkAsync(
      std::string_view keyClass,
      std::string_view keyValue) {
    if (UNLIKELY(enabled_)) {
      return checkAsyncImpl(keyClass, keyValue);
    }
    return folly::unit;
  }

  /**
   * Check a fault, using a dynamically constructed key.
   *
   * This helper method checks for a fault using multiple arguments to construct
   * the key value.  The value arguments are converted to strings using
   * fmt and joined with ", " as the delimiter.  e.g., calling check("myFault",
   * "foo", "bar") will use "foo, bar" as the key.
   *
   * This string construction is only done if fault injection is enabled,
   * and so has no extra overhead if fault injection is disabled.
   */
  template <typename... Args>
  void check(std::string_view keyClass, Args&&... args) {
    if (UNLIKELY(enabled_)) {
      checkImpl(keyClass, constructKey(std::forward<Args>(args)...));
    }
  }
  template <typename... Args>
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> checkAsync(
      std::string_view keyClass,
      Args&&... args) {
    if (UNLIKELY(enabled_)) {
      return checkAsyncImpl(
          keyClass, constructKey(std::forward<Args>(args)...));
    }
    return folly::unit;
  }

  /**
   * Inject a fault that triggers an exception to be thrown.
   *
   * Faults are evaluated in the order in which they are inserted.  If multiple
   * injected faults match a given check, the fault that was injected first
   * takes precedence.
   *
   * The count parameter specifies how many check() calls this fault should
   * match before expiring.  If this is 0 the fault will never expire on its
   * own, and can only be removed by a subsequent call to removeFault().
   */
  void injectError(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      folly::exception_wrapper error,
      size_t count = 0);

  /**
   * Inject a fault that causes the check call to block until explicitly
   * unblocked with a later call to unblock() or unblockWithError()
   */
  void injectBlock(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      size_t count = 0);

  /**
   * Inject a fault that causes the check call to block for a specific amount of
   * time before automatically continuing.
   */
  void injectDelay(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      std::chrono::milliseconds duration,
      size_t count = 0);
  void injectDelayedError(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      std::chrono::milliseconds duration,
      folly::exception_wrapper error,
      size_t count = 0);

  /**
   * Inject a fault that causes the process to exit without cleanup.
   */
  void injectKill(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      size_t count = 0);

  /**
   * Inject a dummy fault that does not trigger any error.
   *
   * One use for this would be inserting a higher-priority no-op before some
   * other fault.  E.g., using a no-op to cause success even if a lower-priority
   * fault would trigger an error.  Another potential use would be a no-op
   * fault that expires after hit a certain number of times, allowing the first
   * N calls to succeed before falling through to a lower priority fault
   * afterwards.
   */
  void injectNoop(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      size_t count = 0);

  /**
   * Remove a previously configured fault definition.
   *
   * The keyValueRegex string must exactly match the regular expression string
   * given to one of the inject*() methods when the fault was defined.
   * If multiple faults have been defined with the given key class and value
   * information only the first one will be removed.  (The one defined
   * earliest.)
   *
   * Returns true if a fault was removed, or false if no fault was defined with
   * the specified key information.
   */
  bool removeFault(std::string_view keyClass, std::string_view keyValueRegex);

  /**
   * Unblock pending check()/checkAsync() calls waiting on a block fault.
   *
   * The keyValueRegex string does not need to match the initial matched fault.
   * For example, you can define a block fault for ".*", and then later unblock
   * just a subset of the check calls pending on this fault.
   */
  size_t unblock(std::string_view keyClass, std::string_view keyValueRegex);
  size_t unblockWithError(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      folly::exception_wrapper error);
  size_t unblockAll();
  size_t unblockAllWithError(folly::exception_wrapper error);

 private:
  struct Block {};
  struct Delay {
    explicit Delay(std::chrono::milliseconds d) : duration(d) {}
    Delay(std::chrono::milliseconds d, folly::exception_wrapper e)
        : duration(d), error{std::move(e)} {}

    std::chrono::milliseconds duration;
    std::optional<folly::exception_wrapper> error;
  };
  struct Kill {};

  using FaultBehavior = std::variant<
      folly::Unit, // no fault
      Block, // block until explicitly unblocked at a later point
      Delay, // delay for a specified amount of time
      folly::exception_wrapper, // throw an exception
      Kill // exit the process ungracefully
      >;
  struct Fault {
    Fault(std::string_view regex, FaultBehavior&& behavior, size_t count);

    // A regular expression for the key values that this fault matches
    boost::regex keyValueRegex;
    // The number of remaining times this fault may be triggered.
    // If this is 0 then this fault can be triggered indefinitely.
    size_t countRemaining{0};
    FaultBehavior behavior;
  };
  struct BlockedCheck {
    BlockedCheck(std::string_view kv, folly::Promise<folly::Unit>&& p)
        : keyValue{kv}, promise{std::move(p)} {}

    std::string keyValue;
    folly::Promise<folly::Unit> promise;
  };

  struct State {
    // A map from key class -> Faults
    folly::F14NodeMap<std::string, std::vector<Fault>> faults;
    // A map from key class -> BlockedChecks
    folly::F14NodeMap<std::string, std::vector<BlockedCheck>> blockedChecks;
  };

  template <typename... Args>
  std::string constructKey(const Args&... args) {
    return fmt::to_string(
        fmt::join(std::make_tuple<const Args&...>(args...), ", "));
  }

  FOLLY_NODISCARD ImmediateFuture<folly::Unit> checkAsyncImpl(
      std::string_view keyClass,
      std::string_view keyValue);
  void checkImpl(std::string_view keyClass, std::string_view keyValue);

  void injectFault(
      std::string_view keyClass,
      std::string_view keyValueRegex,
      FaultBehavior&& fault,
      size_t count);
  FaultBehavior findFault(std::string_view keyClass, std::string_view keyValue);

  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> addBlockedFault(
      std::string_view keyClass,
      std::string_view keyValue);
  FOLLY_NODISCARD std::vector<BlockedCheck> extractBlockedChecks(
      std::string_view keyClass,
      std::string_view keyValueRegex);
  size_t unblockAllImpl(std::optional<folly::exception_wrapper> error);

  /**
   * Fault injection is normally disabled during normal production use.
   * This simple constant flag allows us to quickly check if fault injection is
   * enabled in the first place, and fall through
   */
  bool const enabled_{false};
  folly::Synchronized<State> state_;
};

} // namespace facebook::eden
