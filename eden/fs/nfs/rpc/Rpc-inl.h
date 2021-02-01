/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

// Implementation details of the various macros present in Rpc.h.

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a call to
// the serializer for a given field name
#define EDEN_XDR_SER(name) serializeXdr(xdr, a.name);

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a call to
// the de-serializer for a given field name.
#define EDEN_XDR_DE(name) deSerializeXdrInto(xdr, a.name);

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a comparison
// between this.name and other.name, followed by &&.  It is intended
// to be used in a sequence and have a literal 1 following that sequence.
// It is used to generator the == operator for a type.
// It is present primarily for testing purposes.
#define EDEN_XDR_EQ(name) name == other.name&&

/* --- Helpers for EDEN_XDR_FOR_EACH_PAIR --- */

// like FOLLY_PP_DETAIL_NARGS_1 but copes with more arguments
#define EDEN_XDR_DETAIL_NARGS_1( \
    dummy,                       \
    _16,                         \
    _15,                         \
    _14,                         \
    _13,                         \
    _12,                         \
    _11,                         \
    _10,                         \
    _9,                          \
    _8,                          \
    _7,                          \
    _6,                          \
    _5,                          \
    _4,                          \
    _3,                          \
    _2,                          \
    _1,                          \
    _0,                          \
    ...)                         \
  _0

// like FOLLY_PP_DETAIL_NARGS but copes with more arguments
#define EDEN_XDR_DETAIL_NARGS(...) \
  EDEN_XDR_DETAIL_NARGS_1(         \
      dummy,                       \
      ##__VA_ARGS__,               \
      16,                          \
      15,                          \
      14,                          \
      13,                          \
      12,                          \
      11,                          \
      10,                          \
      9,                           \
      8,                           \
      7,                           \
      6,                           \
      5,                           \
      4,                           \
      3,                           \
      2,                           \
      1,                           \
      0)

#define EDEN_XDR_FOR_EACH_PAIR_REC_0(fn, ...)
#define EDEN_XDR_FOR_EACH_PAIR_REC_2(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_0(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_4(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_2(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_6(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_4(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_8(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_6(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_10(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_8(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_12(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_10(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_14(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_12(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_REC_16(fn, a, b, ...) \
  fn(a, b) EDEN_XDR_FOR_EACH_PAIR_REC_14(fn, __VA_ARGS__)

#define EDEN_XDR_FOR_EACH_PAIR_2(fn, n, ...) \
  EDEN_XDR_FOR_EACH_PAIR_REC_##n(fn, __VA_ARGS__)
#define EDEN_XDR_FOR_EACH_PAIR_1(fn, n, ...) \
  EDEN_XDR_FOR_EACH_PAIR_2(fn, n, __VA_ARGS__)

/* ---- */

// Similar to FOLLY_PP_FOR_EACH, except that it passes pairs of arguments.
// For example:
// EDEN_XDR_FOR_EACH_PAIR(MACRO, a1, a2, b1, b2)
// will expand to:
// MACRO(a1, a2)
// MACRO(b1, b2)
//
#define EDEN_XDR_FOR_EACH_PAIR(fn, ...) \
  EDEN_XDR_FOR_EACH_PAIR_1(fn, EDEN_XDR_DETAIL_NARGS(__VA_ARGS__), __VA_ARGS__)

// A helper that emits a variant visitor fragment for a given
// `value` (which is intended to be an enum variant such as
// `reject_stat::RPC_MISMATCH`) and its corresponding variant
// type `ty` eg: `mismatch_info`.  This visitor is intended
// to serialize the variant with its tag.
// The implicit `arg` must be the name of the lambda argument passed to
// `std::visit`.
#define EDEN_XDR_VAR_SER(value, ty)      \
  if constexpr (std::is_same_v<T, ty>) { \
    serializeXdr(xdr, value);            \
    serializeXdr(xdr, arg);              \
  } else

// A helper to emit the deserialization part
// Similarly to EDEN_XDR_VAR_SER, `v` is an implicit argument that represents
// the output argument to the deserialization function.
#define EDEN_XDR_VAR_DE(value, ty) \
  case value: {                    \
    ty t;                          \
    deSerializeXdrInto(xdr, t);    \
    v.v = std::move(t);            \
    break;                         \
  }

// A helper to emit the setters and getters. `v` and `tag` represent the
// variant and the enum variant.
#define EDEN_XDR_VAR_ACCESSOR_IMPL(value, ty) \
  void set_##value(ty&& t) {                  \
    v = std::move(t);                         \
    tag = value;                              \
  }                                           \
  const ty& get_##value() const {             \
    return std::get<ty>(v);                   \
  }                                           \
  ty& get_##value() {                         \
    return std::get<ty>(v);                   \
  }

// A helper to build a comma-separated list of the types that make up the
// variant. A `std::monostate` is used to terminate the list.
#define EDEN_XDR_VAR_TYPE_FROM_PAIR(value, ty) ty,
#define EDEN_XDR_VAR_TYPES(...)                                    \
  EDEN_XDR_FOR_EACH_PAIR(EDEN_XDR_VAR_TYPE_FROM_PAIR, __VA_ARGS__) \
  std::monostate
