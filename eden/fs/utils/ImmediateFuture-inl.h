/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/lang/Assume.h>

namespace facebook::eden {

template <typename T>
void ImmediateFuture<T>::destroy() {
  switch (kind_) {
    case Kind::Immediate:
      using TryType = folly::Try<T>;
      immediate_.~TryType();
      break;
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      using SemiFutureType = folly::SemiFuture<T>;
      semi_.~SemiFutureType();
      break;
    case Kind::Nothing:
      break;
  }
  kind_ = Kind::Nothing;
}

template <typename T>
ImmediateFuture<T>::ImmediateFuture(folly::Try<T>&& value) noexcept(
    std::is_nothrow_move_constructible_v<folly::Try<T>>) {
  if (detail::kImmediateFutureAlwaysDefer) {
    kind_ = Kind::SemiFuture;
    new (&semi_) folly::SemiFuture<T>{std::move(value)};
  } else {
    kind_ = Kind::Immediate;
    new (&immediate_) folly::Try<T>{std::move(value)};
  }
}

template <typename T>
ImmediateFuture<T>::ImmediateFuture(
    folly::SemiFuture<T>&& fut,
    SemiFutureReadiness readiness) noexcept {
  if (readiness == SemiFutureReadiness::LazySemiFuture) {
    kind_ = Kind::LazySemiFuture;
    new (&semi_) folly::SemiFuture<T>{std::move(fut)};
  } else if (!fut.isReady() || detail::kImmediateFutureAlwaysDefer) {
    kind_ = Kind::SemiFuture;
    new (&semi_) folly::SemiFuture<T>{std::move(fut)};
  } else {
    kind_ = Kind::Immediate;
    new (&immediate_) folly::Try<T>{std::move(fut).getTry()};
  }
}

template <typename T>
ImmediateFuture<T>::~ImmediateFuture() {
  destroy();
}

template <typename T>
ImmediateFuture<T>::ImmediateFuture(ImmediateFuture<T>&& other) noexcept(
    std::is_nothrow_move_constructible_v<folly::Try<T>>)
    : kind_(other.kind_) {
  switch (kind_) {
    case Kind::Immediate:
      new (&immediate_) folly::Try<T>(std::move(other.immediate_));
      break;
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      new (&semi_) folly::SemiFuture<T>(std::move(other.semi_));
      break;
    case Kind::Nothing:
      break;
  }
  other.kind_ = Kind::Nothing;
}

template <typename T>
ImmediateFuture<T>&
ImmediateFuture<T>::operator=(ImmediateFuture<T>&& other) noexcept(
    std::is_nothrow_move_constructible_v<folly::Try<T>>) {
  if (this == &other) {
    return *this;
  }
  destroy();
  switch (other.kind_) {
    case Kind::Immediate:
      new (&immediate_) folly::Try<T>(std::move(other.immediate_));
      break;
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      new (&semi_) folly::SemiFuture<T>(std::move(other.semi_));
      break;
    case Kind::Nothing:
      break;
  }
  kind_ = other.kind_;
  other.kind_ = Kind::Nothing;
  return *this;
}

namespace detail {
template <typename Func, typename... Args>
ImmediateFuture<detail::continuation_result_t<Func, Args...>>
makeImmediateFutureFromImmediate(Func&& func, Args... args) {
  using NewType = detail::continuation_result_t<Func, Args...>;
  using FuncRetType = std::invoke_result_t<Func, Args...>;

  try {
    // In the case where Func returns void, force the return value to
    // be folly::unit.
    if constexpr (std::is_same_v<FuncRetType, void>) {
      func(std::forward<Args>(args)...);
      return folly::unit;
    } else {
      return func(std::forward<Args>(args)...);
    }
  } catch (...) {
    return folly::Try<NewType>(
        folly::exception_wrapper{std::current_exception()});
  }
}
} // namespace detail

template <typename T>
template <typename Func>
ImmediateFuture<detail::continuation_result_t<Func, T>>
ImmediateFuture<T>::thenValue(Func&& func) && {
  using RetType = detail::continuation_result_t<Func, T>;
  if (kind_ == Kind::Immediate && immediate_.hasException()) {
    return ImmediateFuture<RetType>{
        folly::Try<RetType>{std::move(immediate_).exception()}};
  }

  return std::move(*this).thenTry(
      [func = std::forward<Func>(func)](
          folly::Try<T>&& try_) mutable -> ImmediateFuture<RetType> {
        if (try_.hasValue()) {
          return detail::makeImmediateFutureFromImmediate(
              std::move(func), std::move(try_).value());
        } else {
          return folly::Try<RetType>(std::move(try_).exception());
        }
      });
}

template <typename T>
template <typename Func>
ImmediateFuture<T> ImmediateFuture<T>::thenError(Func&& func) && {
  if (kind_ == Kind::Immediate && immediate_.hasValue()) {
    return std::move(*this).immediate_;
  }

  return std::move(*this).thenTry(
      [func = std::forward<Func>(func)](
          folly::Try<T>&& try_) mutable -> ImmediateFuture<T> {
        if (try_.hasException()) {
          return detail::makeImmediateFutureFromImmediate(
              std::move(func), std::move(try_).exception());
        } else {
          return ImmediateFuture{std::move(try_)};
        }
      });
}

template <typename T>
template <typename Func>
ImmediateFuture<T> ImmediateFuture<T>::ensure(Func&& func) && {
  return std::move(*this).thenTry(
      [func = std::forward<Func>(func)](
          folly::Try<T> try_) mutable -> folly::Try<T> {
        func();
        return try_;
      });
}

template <typename T>
ImmediateFuture<folly::Unit> ImmediateFuture<T>::unit() && {
  return std::move(*this).thenValue([](T&&) {});
}

template <typename T>
bool ImmediateFuture<T>::isReady() const {
  switch (kind_) {
    case Kind::Immediate:
      return true;
    case Kind::SemiFuture:
      if (detail::kImmediateFutureAlwaysDefer) {
        return false;
      }
      return semi_.isReady();
    case Kind::LazySemiFuture:
      return false;
    case Kind::Nothing:
      throw DestroyedImmediateFutureError{};
  }
  folly::assume_unreachable();
}

template <typename T>
template <typename Func>
ImmediateFuture<detail::continuation_result_t<Func, folly::Try<T>>>
ImmediateFuture<T>::thenTry(Func&& func) && {
  using FuncRetType = std::invoke_result_t<Func, folly::Try<T>>;

  if (isReady()) {
    return detail::makeImmediateFutureFromImmediate(
        std::forward<Func>(func), std::move(*this).getTry());
  } else {
    // In the case where Func returns an ImmediateFuture, we need to
    // transform that return value into a SemiFuture so that the return
    // type is a SemiFuture<> and not a SemiFuture<ImmediateFuture<>>.
    auto semiFut = std::move(*this).semi().defer(std::forward<Func>(func));
    if constexpr (detail::isImmediateFuture<FuncRetType>::value) {
      return std::move(semiFut).deferValue(
          [](auto&& immFut) { return std::move(immFut).semi(); });
    } else {
      return std::move(semiFut);
    }
  }
}

template <typename T>
T ImmediateFuture<T>::get() && {
  switch (kind_) {
    case Kind::Immediate:
      return std::move(immediate_).value();
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      return std::move(semi_).get();
    case Kind::Nothing:
      throw DestroyedImmediateFutureError();
  }
  folly::assume_unreachable();
}

template <typename T>
folly::Try<T> ImmediateFuture<T>::getTry() && {
  switch (kind_) {
    case Kind::Immediate:
      return std::move(immediate_);
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      return std::move(semi_).getTry();
    case Kind::Nothing:
      throw DestroyedImmediateFutureError();
  }
  folly::assume_unreachable();
}

template <typename T>
T ImmediateFuture<T>::get(folly::HighResDuration timeout) && {
  switch (kind_) {
    case Kind::Immediate:
      return std::move(immediate_).value();
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      return std::move(semi_).get(timeout);
    case Kind::Nothing:
      throw DestroyedImmediateFutureError();
  }
  folly::assume_unreachable();
}

template <typename T>
folly::Try<T> ImmediateFuture<T>::getTry(folly::HighResDuration timeout) && {
  switch (kind_) {
    case Kind::Immediate:
      return std::move(immediate_);
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      return std::move(semi_).getTry(timeout);
    case Kind::Nothing:
      throw DestroyedImmediateFutureError();
  }
  folly::assume_unreachable();
}

template <typename T>
folly::SemiFuture<T> ImmediateFuture<T>::semi() && {
  switch (kind_) {
    case Kind::Immediate:
      return std::move(immediate_);
    case Kind::SemiFuture:
    case Kind::LazySemiFuture:
      return std::move(semi_);
    case Kind::Nothing:
      throw DestroyedImmediateFutureError();
  }
  folly::assume_unreachable();
}

template <typename T, typename E>
typename std::
    enable_if_t<std::is_base_of<std::exception, E>::value, ImmediateFuture<T>>
    makeImmediateFuture(E const& e) {
  return ImmediateFuture<T>{folly::Try<T>{e}};
}

template <typename T>
ImmediateFuture<T> makeImmediateFuture(folly::exception_wrapper e) {
  return ImmediateFuture<T>{folly::Try<T>{std::move(e)}};
}

template <typename Func>
auto makeImmediateFutureWith(Func&& func) {
  return detail::makeImmediateFutureFromImmediate(std::forward<Func>(func));
}

template <typename T>
ImmediateFuture<std::vector<folly::Try<T>>> collectAll(
    std::vector<ImmediateFuture<T>> futures) {
  std::vector<folly::SemiFuture<T>> semis;
  std::vector<size_t> semisIndices;
  std::vector<folly::Try<T>> res;
  res.reserve(futures.size());

  size_t currentIndex = 0;
  for (auto& fut : futures) {
    if (fut.isReady()) {
      res.emplace_back(std::move(fut).getTry());
    } else {
      semis.emplace_back(std::move(fut).semi());
      semisIndices.push_back(currentIndex);
      res.emplace_back(
          folly::Try<T>{std::logic_error("Uncompleted SemiFuture")});
    }
    currentIndex++;
  }

  if (semis.empty()) {
    // All the ImmediateFuture were immediate, let's return an ImmediateFuture
    // that holds an immediate vector too.
    return std::move(res);
  }

  return folly::collectAll(std::move(semis))
      .deferValue(
          [res = std::move(res), semisIndices = std::move(semisIndices)](
              std::vector<folly::Try<T>> semisRes) mutable {
            for (size_t i = 0; i < semisRes.size(); i++) {
              res[semisIndices[i]] = std::move(semisRes[i]);
            }
            return std::move(res);
          });
}

template <typename T>
ImmediateFuture<std::vector<T>> collectAllSafe(
    std::vector<ImmediateFuture<T>> futures) {
  return facebook::eden::collectAll(std::move(futures))
      .thenValue(
          [](std::vector<folly::Try<T>> futures) -> folly::Try<std::vector<T>> {
            std::vector<T> res;
            res.reserve(futures.size());

            for (auto& try_ : futures) {
              if (try_.hasException()) {
                return folly::Try<std::vector<T>>{try_.exception()};
              }
              res.push_back(std::move(try_).value());
            }
            return folly::Try{res};
          });
}

template <typename... Fs>
ImmediateFuture<
    std::tuple<folly::Try<typename folly::remove_cvref_t<Fs>::value_type>...>>
collectAll(Fs&&... fs) {
  using Result =
      std::tuple<folly::Try<typename folly::remove_cvref_t<Fs>::value_type>...>;
  struct Context {
    ~Context() {
      p.setValue(std::move(results));
    }
    folly::Promise<Result> p;
    Result results;
  };

  std::vector<folly::SemiFuture<folly::Unit>> semis;

  // TODO: fast-path the case where everything is ready and avoid allocations
  // entirely.
  auto ctx = std::make_shared<Context>();
  folly::futures::detail::foreach(
      [&](auto i, auto&& f) {
        if (f.isReady()) {
          std::get<i.value>(ctx->results) = std::move(f).getTry();
        } else {
          semis.emplace_back(std::move(f).semi().defer([i, ctx](auto&& t) {
            std::get<i.value>(ctx->results) = std::move(t);
          }));
        }
      },
      static_cast<Fs&&>(fs)...);

  if (semis.empty()) {
    // Since all the ImmediateFuture were ready, the Context hasn't been
    // copied to any lambdas, and thus will be destroyed once this lambda
    // returns. This will make the returned SemiFuture ready which the
    // ImmediateFuture constructor will extract the value from.
    auto fut = ctx->p.getSemiFuture();
    ctx.reset();
    return fut;
  }

  return folly::collectAll(std::move(semis)).deferValue([ctx](auto&&) {
    return ctx->p.getSemiFuture();
  });
}

template <typename... Fs>
ImmediateFuture<std::tuple<typename folly::remove_cvref_t<Fs>::value_type...>>
collectAllSafe(Fs&&... fs) {
  return facebook::eden::collectAll(static_cast<Fs&&>(fs)...)
      .thenValue(
          [](std::tuple<
              folly::Try<typename folly::remove_cvref_t<Fs>::value_type>...>&&
                 res) { return unwrapTryTuple(std::move(res)); });
}

} // namespace facebook::eden
