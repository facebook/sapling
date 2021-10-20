# Futures and Asynchronous Code

This document assumes some working knowledge of folly::Future and folly::SemiFuture. Please read the [Future overview](https://github.com/facebook/folly/blob/master/folly/docs/Futures.md) first.

## Why Future?

EdenFS is largely concurrent and asynchronous. The traditional way to write this kind of code would be explicit state machines with requests and callbacks. It's easy to forget to call a callback or call one twice under rarely-executed paths like error handling.

To make asynchronous code easier to reason about, Folly provides `folly::Future` and `folly::Promise`. Each Future and Promise form a pair, where `folly::Future` holds the eventual value and Promise is how the value is published. Readers can either block on the result (offering their thread to any callbacks that may run) or schedule a callback to be run when the value is available. `folly::Promise` is fulfilled on the writing side.

## Why SemiFuture?

The biggest problem with Future is that callbacks may run either on the thread calling `Future::then` or on the thread calling `Promise::set`. Callbacks have to be written carefully, and if they acquire locks, any site that calls `Future::then` or `Promise::set` must not hold those locks.

`folly::SemiFuture` is a reaction to these problems. It's a Future without a `SemiFuture::then` method. Assuming no use of unsafe APIs (including any InlineExecutor), callbacks will never run on the thread that calls `Promise::set`. Any system with an internal thread pool that cannot tolerate arbitrary callbacks running on its threads should use `SemiFuture`.

## Why ImmediateFuture?

Future and SemiFuture introduce significant overhead. A Future/Promise pair hold a heap-allocated, atomic refcounted FutureCore. In EdenFS, it's common to make an asynchronous call that hits cache and can answer immediately. Heap allocating the result is comparatively expensive. We introduced `facebook::eden::ImmediateFuture` for those cases. ImmediateFuture either stores the result value inline or holds a SemiFuture.

## When should I use which Future?

There are reasons to use each Future.

&nbsp; | Future | SemiFuture | ImmediateFuture
---    | ---    | ---        | ---
Storage is heap-allocated | yes | yes | no
Callbacks run as early as the result is available | yes | no | no
Callbacks may run on the fulfiller's thread | yes | no | no
Callbacks may run immediately or asynchronously | yes | no | yes
sizeof, cost of move() | void* | void* | Depends on sizeof(T) with minimum of 40 bytes as of Oct 2021

Future should be used when it's important the callback runs as early as possible. For example, measuring the duration of internal operations.

SemiFuture or ImmediateFuture should be used when it's important that chained callbacks never run on internal thread pools.

ImmediateFuture should be used when the value is small and avoiding an allocation is important for performance. Large structs can use unique_ptr or shared_ptr.

It's important to note that, when a callback and its closures hold reference counts or are larger than the result value, it can be worth using Future, because the callbacks are collapsed into a value as early as possible. SemiFuture, even if the SemiFuture is held by an ImmediateFuture, will not collapse any chained callbacks until the SemiFuture is attached to an executor.

## TODO

* Unsafely mapping ImmediateFuture onto Future with .via(QueuedImmediateExecutor)?
* What about coroutines?
