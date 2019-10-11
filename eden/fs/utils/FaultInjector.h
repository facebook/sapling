/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <boost/regex.hpp>
#include <boost/variant.hpp>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include <folly/futures/Future.h>
#include <optional>

namespace facebook {
namespace eden {

/**
 * A helper class for injecting artificial faults into the normal program flow.
 *
 * This allows external test code to inject delay or failures into specific
 * locations in the program.
 */
class FaultInjector {
 public:
  explicit FaultInjector(bool enabled);
  ~FaultInjector();

  /**
   * Check for an injected fault with the specified key.
   *
   * If fault injection is disabled or if no fault matching this key has been
   * injected this returns a SemiFuture that is immediately ready.
   *
   * If a fault matching this key has been injected this may return a future
   * that blocks until some later point, and/or that may fail with an
   * artificially injected error.
   *
   * The main reason for having keyClass and keyValue as 2 separate string
   * parameters is that this often allows us to avoid having to perform string
   * allocation when checking for faults.  For many faults the keyClass will be
   * a fixed string literal, while the keyValue will be some runtime-specified
   * string.  If we accepted only a single key parameter these two strings would
   * need to be joined at runtime when checking for faults.
   */
  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> checkAsync(
      folly::StringPiece keyClass,
      folly::StringPiece keyValue) {
    if (UNLIKELY(enabled_)) {
      return checkAsyncImpl(keyClass, keyValue);
    }
    return folly::makeSemiFuture();
  }

  /**
   * Check for an injected fault with the specified key.
   *
   * This API always returns or throws an exception immediately, and therefore
   * does not allow faults that block or delay execution.  However, because it
   * does not use SemiFuture it is lower-overhead for situations where you
   * simply want the ability to inject an exception in performance-sensitive
   * code.
   */
  void check(folly::StringPiece keyClass, folly::StringPiece keyValue) {
    if (UNLIKELY(enabled_)) {
      return checkImpl(keyClass, keyValue);
    }
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
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
      folly::exception_wrapper error,
      size_t count = 0);

  /**
   * Inject a fault that causes the check call to block until explicitly
   * unblocked with a later call to unblock() or unblockWithError()
   */
  void injectBlock(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
      size_t count = 0);

  /**
   * Inject a fault that causes the check call to block for a specific amount of
   * time before automatically continuing.
   */
  void injectDelay(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
      std::chrono::milliseconds duration,
      size_t count = 0);
  void injectDelayedError(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
      std::chrono::milliseconds duration,
      folly::exception_wrapper error,
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
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
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
  bool removeFault(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex);

  /**
   * Unblock pending check()/checkAsync() calls waiting on a block fault.
   *
   * The keyValueRegex string does not need to match the initial matched fault.
   * For example, you can define a block fault for ".*", and then later unblock
   * just a subset of the check calls pending on this fault.
   */
  size_t unblock(folly::StringPiece keyClass, folly::StringPiece keyValueRegex);
  size_t unblockWithError(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
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

  using FaultBehavior = boost::variant<
      folly::Unit, // no fault
      Block, // block until explicitly unblocked at a later point
      Delay, // delay for a specified amount of time
      folly::exception_wrapper // throw an exception
      >;
  struct Fault {
    Fault(folly::StringPiece regex, FaultBehavior&& behavior, size_t count);

    // A regular expression for the key values that this fault matches
    boost::regex keyValueRegex;
    // The number of remaining times this fault may be triggered.
    // If this is 0 then this fault can be triggered indefinitely.
    size_t countRemaining{0};
    FaultBehavior behavior;
  };
  struct BlockedCheck {
    BlockedCheck(folly::StringPiece kv, folly::Promise<folly::Unit>&& p)
        : keyValue(kv.str()), promise(std::move(p)) {}

    std::string keyValue;
    folly::Promise<folly::Unit> promise;
  };

  struct State {
    // A map from key class -> Faults
    folly::StringKeyedUnorderedMap<std::vector<Fault>> faults;
    // A map from key class -> BlockedChecks
    folly::StringKeyedUnorderedMap<std::vector<BlockedCheck>> blockedChecks;
  };

  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> checkAsyncImpl(
      folly::StringPiece keyClass,
      folly::StringPiece keyValue);
  void checkImpl(folly::StringPiece keyClass, folly::StringPiece keyValue);

  void injectFault(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex,
      FaultBehavior&& fault,
      size_t count);
  FaultBehavior findFault(
      folly::StringPiece keyClass,
      folly::StringPiece keyValue);

  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> addBlockedFault(
      folly::StringPiece keyClass,
      folly::StringPiece keyValue);
  FOLLY_NODISCARD std::vector<BlockedCheck> extractBlockedChecks(
      folly::StringPiece keyClass,
      folly::StringPiece keyValueRegex);
  size_t unblockAllImpl(std::optional<folly::exception_wrapper> error);

  /**
   * Fault injection is normally disabled during normal production use.
   * This simple constant flag allows us to quickly check if fault injection is
   * enabled in the first place, and fall through
   */
  bool const enabled_{false};
  folly::Synchronized<State> state_;
};

} // namespace eden
} // namespace facebook
