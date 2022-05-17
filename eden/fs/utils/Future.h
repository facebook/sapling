/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>

namespace facebook::eden {

/**
 * Same semantics as folly::collect, but does not return until all futures are
 * completed. folly::collect completes its future when any of its input futures
 * completes with an error. This is unsafe in the following example:
 *
 * struct C {
 *   Future<int> internal1();
 *   Future<int> internal2();
 *   Future<int> method() {
 *     return folly::collect(internal1(), internal2())
 *       .thenValue([](std::tuple<int, int> results) {
 *         return std::get<0>(results) + std::get<1>(results);
 *       });
 *   }
 * };
 *
 * Using collectSafe makes the above example legal.
 */
template <typename... Fs>
folly::Future<std::tuple<typename folly::remove_cvref_t<Fs>::value_type...>>
collectSafe(Fs&&... fs) {
  using namespace folly;

  using Result = std::tuple<typename remove_cvref_t<Fs>::value_type...>;
  struct Context {
    Promise<Result> p;

    folly::exception_wrapper exception;
    std::atomic<bool> hasException{false};

    // It would be slightly more size-efficient to use std::optional here, but
    // folly::Try provides unwrapTryTuple.
    std::tuple<Try<typename remove_cvref_t<Fs>::value_type>...> results;

    // count should match the ctx shared_ptr reference count, but managing
    // our own count avoids a std::terminate if an exception is thrown during
    // unwinding, and ensures `p` is completed on the same thread as the last
    // future's executor, no matter what thread drops the callback function.
    std::atomic<size_t> count{sizeof...(fs)};

    void saveException(folly::exception_wrapper ew) noexcept {
      if (!hasException.exchange(true, std::memory_order_acq_rel)) {
        exception = std::move(ew);
      }
    }

    void decref() {
      if (1 != count.fetch_sub(1, std::memory_order_acq_rel)) {
        return;
      }

      if (hasException.load(std::memory_order_acquire)) {
        p.setException(std::move(exception));
      } else {
        p.setValue(unwrapTryTuple(std::move(results)));
      }
    }
  };

  auto ctx = std::make_shared<Context>();
  futures::detail::foreach(
      [&](auto i, auto&& f) {
        auto fut = std::move(f);
        fut.setCallback_([i, ctx](Executor::KeepAlive<>&&, auto&& t) {
          // Every operation before decref() should be noexcept.
          if (t.hasException()) {
            ctx->saveException(std::move(t.exception()));
          } else {
            std::get<i.value>(ctx->results).emplace(std::move(t.value()));
          }
          ctx->decref();
        });
      },
      static_cast<Fs&&>(fs)...);
  return ctx->p.getFuture();
}

} // namespace facebook::eden
