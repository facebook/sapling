/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <folly/Preprocessor.h>
#include <folly/io/Cursor.h>
#include <optional>
#include <variant>

// https://tools.ietf.org/html/rfc4506

// This is a macro that is used to emit the implementation of XDR serialization,
// deserialization and operator== for a type.
//
// The parameters the type name followed by the list of field names.
// The field names must be listed in the same order as the RPC/XDR
// definition for the type requires.  It is good practice to have that
// order match the order of the fields in the struct.
//
// Example: in the header file:
//
// struct Foo {
//    int bar;
//    int baz;
// };
// EDEN_XDR_SERDE_DECL(Foo);
//
// Then in the cpp file:
//
// EDEN_XDR_SERDE_IMPL(Foo, bar, baz);

// This macro declares the XDR serializer and deserializer functions
// for a given type.
// See EDEN_XDR_SERDE_IMPL above for an example.
#define EDEN_XDR_SERDE_DECL(STRUCT, ...)                      \
  bool operator==(const STRUCT& a, const STRUCT& b);          \
  template <>                                                 \
  struct XdrTrait<STRUCT> {                                   \
    static void serialize(                                    \
        folly::io::QueueAppender& appender,                   \
        const STRUCT& a) {                                    \
      FOLLY_PP_FOR_EACH(EDEN_XDR_SER, __VA_ARGS__)            \
    }                                                         \
    static STRUCT deserialize(folly::io::Cursor& cursor) {    \
      STRUCT ret;                                             \
      FOLLY_PP_FOR_EACH(EDEN_XDR_DE, __VA_ARGS__)             \
      return ret;                                             \
    }                                                         \
    static size_t serializedSize(const STRUCT& a) {           \
      return FOLLY_PP_FOR_EACH(EDEN_XDR_SIZE, __VA_ARGS__) 0; \
    }                                                         \
  }

#define EDEN_XDR_SERDE_IMPL(STRUCT, ...)                  \
  bool operator==(const STRUCT& a, const STRUCT& b) {     \
    return FOLLY_PP_FOR_EACH(EDEN_XDR_EQ, __VA_ARGS__) 1; \
  }

// Implementation details for the macros above:

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a call to
// the serializer for a given field name
#define EDEN_XDR_SER(name) \
  XdrTrait<decltype(a.name)>::serialize(appender, a.name);

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a call to
// the de-serializer for a given field name.
#define EDEN_XDR_DE(name) \
  ret.name = XdrTrait<decltype(ret.name)>::deserialize(cursor);

// This is a helper called by FOLLY_PP_FOR_EACH. It computes the serialized
// size of the given field.
#define EDEN_XDR_SIZE(name) XdrTrait<decltype(a.name)>::serializedSize(a.name) +

// This is a helper called by FOLLY_PP_FOR_EACH. It emits a comparison
// between a.name and b.name, followed by &&.  It is intended
// to be used in a sequence and have a literal 1 following that sequence.
// It is used to generator the == operator for a type.
// It is present primarily for testing purposes.
#define EDEN_XDR_EQ(name) a.name == b.name&&

namespace facebook::eden {

/**
 * Trait used to XDR encode a type.
 *
 * A struct that needs serializing will need to implement the 2 functions:
 *
 * `static void serialize(folly::io::QueueAppender& appender, const T& value)`
 * `static T deserialize(folly::io::Cursor& cursor)`
 *
 * Additionally, a 3rd method can be added to compute the size of the
 * serialized struct:
 *
 * `static size_t serializedSize(const T& value)`
 *
 * The encoding follows:
 * https://tools.ietf.org/html/rfc4506
 */
template <typename T, class Enable = void>
struct XdrTrait;

namespace detail {

template <typename T>
struct IsXdrIntegral
    : std::integral_constant<
          bool,
          std::is_same_v<int32_t, T> || std::is_same_v<uint32_t, T> ||
              std::is_same_v<int64_t, T> || std::is_same_v<uint64_t, T> ||
              std::is_same_v<float, T> || std::is_same_v<double, T>> {};

} // namespace detail

/**
 * Integral types are encoded as big-endian.
 */
template <typename T>
struct XdrTrait<T, typename std::enable_if_t<detail::IsXdrIntegral<T>::value>> {
  static void serialize(folly::io::QueueAppender& appender, T value) {
    appender.writeBE<T>(value);
  }

  static T deserialize(folly::io::Cursor& cursor) {
    return cursor.readBE<T>();
  }

  static constexpr size_t serializedSize(const T&) {
    return sizeof(T);
  }
};

/**
 * Boolean values are encoded as a 0/1 integer.
 */
template <>
struct XdrTrait<bool> {
  static void serialize(folly::io::QueueAppender& appender, bool value) {
    XdrTrait<int32_t>::serialize(appender, value ? 1 : 0);
  }

  static bool deserialize(folly::io::Cursor& cursor) {
    return XdrTrait<int32_t>::deserialize(cursor) ? true : false;
  }

  static constexpr size_t serializedSize(const bool) {
    return sizeof(int32_t);
  }
};

/**
 * Enumeration values are encoded as a integer.
 */
template <typename T>
struct XdrTrait<T, typename std::enable_if_t<std::is_enum_v<T>>> {
  static void serialize(folly::io::QueueAppender& appender, const T& value) {
    static_assert(sizeof(T) <= 4, "enum must fit in int32");
    XdrTrait<int32_t>::serialize(appender, static_cast<int32_t>(value));
  }

  static T deserialize(folly::io::Cursor& cursor) {
    return static_cast<T>(XdrTrait<int32_t>::deserialize(cursor));
  }

  static constexpr size_t serializedSize(const T&) {
    return sizeof(int32_t);
  }
};

namespace detail {

/**
 * XDR arrays are 4-bytes aligned, make sure we write and skip these when
 * serializing/deserializing data.
 */
inline constexpr size_t roundUp(size_t value) {
  return (value + 3) & ~3;
}

/**
 * Serialize a fixed size byte array. Their content is serialized as is, padded
 * with NUL bytes to align on a 4-byte boundary.
 */
void serialize_fixed(
    folly::io::QueueAppender& appender,
    folly::ByteRange value);

/**
 * Serialize a variable size byte array. The size of the array is written
 * first, followed by the content of the array, this is also aligned on a
 * 4-byte boundary.
 */
void serialize_variable(
    folly::io::QueueAppender& appender,
    folly::ByteRange value);

/**
 * Serialize an IOBuf chain. This is serialized like a variable sized array,
 * ie: size first, followed by the content and aligned on a 4-byte boundary.
 */
void serialize_iobuf(
    folly::io::QueueAppender& appender,
    const folly::IOBuf& buf);

/**
 * Skip the padding bytes that were written during serialization.
 */
inline void skipPadding(folly::io::Cursor& cursor, size_t len) {
  cursor.skip(roundUp(len) - len);
}

} // namespace detail

/**
 * Array are encoded as a fixed size array with no preceding length indicator.
 */
template <size_t N>
struct XdrTrait<std::array<uint8_t, N>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::array<uint8_t, N>& value) {
    detail::serialize_fixed(appender, folly::ByteRange(value));
  }

  static std::array<uint8_t, N> deserialize(folly::io::Cursor& cursor) {
    std::array<uint8_t, N> ret;
    cursor.pull(ret.data(), N);
    detail::skipPadding(cursor, N);
    return ret;
  }

  static constexpr size_t serializedSize(const std::array<uint8_t, N>&) {
    return N;
  }
};

template <typename T, size_t N>
struct XdrTrait<
    std::array<T, N>,
    typename std::enable_if_t<!std::is_same_v<T, uint8_t>>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::array<T, N>& value) {
    for (const auto& item : value) {
      XdrTrait<T>::serialize(appender, item);
    }
  }

  static std::array<T, N> deserialize(folly::io::Cursor& cursor) {
    std::array<T, N> ret;
    for (auto& item : ret) {
      item = XdrTrait<T>::deserialize(cursor);
    }
    return ret;
  }

  static size_t serializedSize(const std::array<T, N>& value) {
    size_t ret = 0;
    for (const auto& item : value) {
      ret += XdrTrait<T>::serializedSize(item);
    }
    return ret;
  }
};

/**
 * Vectors are encoded as a variable sized array: length, followed by its
 * content.
 */
template <>
struct XdrTrait<std::vector<uint8_t>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::vector<uint8_t>& value) {
    detail::serialize_variable(appender, folly::ByteRange(value));
  }

  static std::vector<uint8_t> deserialize(folly::io::Cursor& cursor) {
    auto len = XdrTrait<uint32_t>::deserialize(cursor);
    std::vector<uint8_t> ret(len);
    cursor.pull(ret.data(), len);
    detail::skipPadding(cursor, len);
    return ret;
  }

  static size_t serializedSize(const std::vector<uint8_t>& value) {
    return XdrTrait<uint32_t>::serializedSize(0) +
        detail::roundUp(value.size());
  }
};

/**
 * IOBuf are encoded as a variable sized array, similarly to a vector. IOBuf
 * should be preferred to a vector when the data to serialize/deserialize is
 * potentially large, a vector would copy all the data, while an IOBuf would
 * clone the existing cursor.
 */
template <>
struct XdrTrait<std::unique_ptr<folly::IOBuf>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::unique_ptr<folly::IOBuf>& buf) {
    detail::serialize_iobuf(appender, *buf);
  }

  static std::unique_ptr<folly::IOBuf> deserialize(folly::io::Cursor& cursor) {
    auto len = XdrTrait<uint32_t>::deserialize(cursor);
    auto ret = std::make_unique<folly::IOBuf>();
    cursor.clone(ret, len);
    detail::skipPadding(cursor, len);
    return ret;
  }

  static size_t serializedSize(const std::unique_ptr<folly::IOBuf>& buf) {
    auto len = buf->computeChainDataLength();
    return XdrTrait<uint32_t>::serializedSize(0) + detail::roundUp(len);
  }
};

template <typename T>
struct XdrTrait<
    std::vector<T>,
    typename std::enable_if_t<!std::is_same_v<T, uint8_t>>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::vector<T>& value) {
    XdrTrait<uint32_t>::serialize(appender, value.size());
    for (const auto& item : value) {
      XdrTrait<T>::serialize(appender, item);
    }
  }

  static std::vector<T> deserialize(folly::io::Cursor& cursor) {
    auto len = XdrTrait<uint32_t>::deserialize(cursor);
    std::vector<T> ret;
    ret.reserve(len);
    for (size_t i = 0; i < len; i++) {
      ret.emplace_back(XdrTrait<T>::deserialize(cursor));
    }
    return ret;
  }

  static size_t serializedSize(const std::vector<T>& value) {
    size_t ret = XdrTrait<uint32_t>::serializedSize(0);
    for (const auto& item : value) {
      ret += XdrTrait<T>::serializedSize(item);
    }
    return ret;
  }
};

/**
 * Strings are encoded in the same way as a vector.
 */
template <>
struct XdrTrait<std::string> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::string& value) {
    detail::serialize_variable(
        appender, folly::ByteRange(folly::StringPiece(value)));
  }

  static std::string deserialize(folly::io::Cursor& cursor) {
    auto len = XdrTrait<uint32_t>::deserialize(cursor);
    auto ret = cursor.readFixedString(len);
    detail::skipPadding(cursor, len);
    return ret;
  }

  static size_t serializedSize(const std::string& value) {
    return XdrTrait<uint32_t>::serializedSize(0) +
        detail::roundUp(value.size());
  }
};

/**
 * Common implementation for XDR discriminated union. Creating a new variant
 * can be done by doing the following:
 *
 * struct MyVariant : public XdrVariant<MyEnum, uint64_t, bool> {
 * };
 *
 * template <>
 * struct XdrTrait<MyVariant> : public XdrTrait<MyVariant::Base> {
 *   static MyVariant deserialize(folly::io::Cursor& cursor) {
 *     // To fill in.
 *   }
 * };
 */
template <typename Enum, class... Vars>
struct XdrVariant {
  Enum tag{};
  std::variant<std::monostate, Vars...> v;

  using Base = XdrVariant<Enum, Vars...>;
};

template <typename Enum, class... Vars>
bool operator==(
    const XdrVariant<Enum, Vars...>& a,
    const XdrVariant<Enum, Vars...>& b) {
  return a.tag == b.tag && a.v == b.v;
}

template <typename Enum, class... Vars>
struct XdrTrait<XdrVariant<Enum, Vars...>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const XdrVariant<Enum, Vars...>& value) {
    XdrTrait<Enum>::serialize(appender, value.tag);
    std::visit(
        [&appender](auto&& arg) {
          using ArgType = std::decay_t<decltype(arg)>;
          if constexpr (!std::is_same_v<ArgType, std::monostate>) {
            XdrTrait<ArgType>::serialize(appender, arg);
          }
        },
        value.v);
  }

  static size_t serializedSize(const XdrVariant<Enum, Vars...>& value) {
    return XdrTrait<Enum>::serializedSize(value.tag) +
        std::visit(
               [](auto&& arg) {
                 using ArgType = std::decay_t<decltype(arg)>;
                 if constexpr (std::is_same_v<ArgType, std::monostate>) {
                   return size_t{0};
                 } else {
                   return XdrTrait<ArgType>::serializedSize(arg);
                 }
               },
               value.v);
  }
};

/**
 * Shorthand for variant with a single boolean case where the TRUE expands onto
 * TrueVariantT. The following XDR definition:
 *
 *     union post_op_fh3 switch (bool handle_follows) {
 *       case TRUE:
 *         nfs_fh3  handle;
 *       case FALSE:
 *         void;
 *     };
 *
 * Can simply be written as:
 *
 *     struct post_op_fh3 : public XdrOptionalVariant<nfs_fh3> {};
 *
 * For non-boolean variant but with a single case, UnionTypeT can be used as
 * the variant tag TrueVariantT will be deserialized when the tag is equal to
 * TestValueV. For instance, the following Xdr definition:
 *
 *     union set_atime switch (time_how set_it) {
 *       case SET_TO_CLIENT_TIME:
 *         nfstime3  atime;
 *       default:
 *         void;
 *     };
 *
 * Can be written as:
 *
 *     struct set_atime : public XdrOptionalVariant<
 *                            nfstime3,
 *                            time_how,
 *                            time_how::SET_TO_CLIENT_TIME> {};
 *
 */
template <
    typename TrueVariantT,
    typename UnionTypeT = bool,
    UnionTypeT TestValueV = true>
struct XdrOptionalVariant : public XdrVariant<UnionTypeT, TrueVariantT> {
  using TrueVariant = TrueVariantT;
  using UnionType = UnionTypeT;
  static constexpr UnionType TestValue = TestValueV;

  XdrOptionalVariant() = default;
  /* implicit */ constexpr XdrOptionalVariant(TrueVariant&& set)
      : XdrVariant<UnionType, TrueVariantT>{TestValue, std::move(set)} {}
};

template <typename T>
struct XdrTrait<
    T,
    std::enable_if_t<std::is_base_of_v<
        XdrOptionalVariant<
            typename T::TrueVariant,
            typename T::UnionType,
            T::TestValue>,
        T>>> : public XdrTrait<typename T::Base> {
  static T deserialize(folly::io::Cursor& cursor) {
    T ret;
    ret.tag = XdrTrait<typename T::UnionType>::deserialize(cursor);
    if (ret.tag == T::TestValue) {
      ret.v = XdrTrait<typename T::TrueVariant>::deserialize(cursor);
    }
    return ret;
  }
};

/**
 * Common implementation for recursive data structures. XDR calls them
 * optional-data and denotes them with *, but they are almost always used to
 * build lists.
 *
 * The following XDR definition:
 *
 *     struct entry3 {
 *        fileid3      fileid;
 *        filename3    name;
 *        cookie3      cookie;
 *        entry3       *nextentry;
 *     };
 *
 *     struct dirlist3 {
 *        entry3       *entries;
 *        bool         eof;
 *     };
 *
 * Can be written as:
 *
 *     struct entry3 {
 *        fileid3      fileid;
 *        filename3    name;
 *        cookie3      cookie;
 *     };
 *
 *     struct dirlist3 {
 *        XdrList<entry3> entries;
 *        bool eof;
 *     };
 */
template <typename T>
struct XdrList {
  std::vector<T> list;
};

/**
 * In spirit, an XdrList can be seen as a std::vector<XdrOptionalVariant<T>>
 * and is serialized and deserialized as such, with the last element being an
 * empty XdrOptionalVariant.
 */
template <typename T>
struct XdrTrait<XdrList<T>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const XdrList<T>& value) {
    for (const auto& element : value.list) {
      XdrTrait<bool>::serialize(appender, true);
      XdrTrait<T>::serialize(appender, element);
    }
    // Terminate the list with an empty element.
    XdrTrait<bool>::serialize(appender, false);
  }

  static XdrList<T> deserialize(folly::io::Cursor& cursor) {
    XdrList<T> res;
    while (true) {
      auto hasNext = XdrTrait<bool>::deserialize(cursor);
      if (!hasNext) {
        // This was the last element.
        return res;
      }
      res.list.push_back(XdrTrait<T>::deserialize(cursor));
    }
  }

  static size_t serializedSize(const XdrList<T>& value) {
    size_t ret = 0;
    for (const auto& element : value.list) {
      ret += XdrTrait<bool>::serializedSize(true);
      ret += XdrTrait<T>::serializedSize(element);
    }
    ret += XdrTrait<bool>::serializedSize(false);
    return ret;
  }
};

template <typename T>
bool operator==(const XdrList<T>& a, const XdrList<T>& b) {
  return a.list == b.list;
}

/**
 * Non-recursive optional data is encoded as a boolean followed by the data if
 * present. For list-like datastructures, prefer using XdrList.
 */
template <typename T>
struct XdrTrait<std::optional<T>> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const std::optional<T>& value) {
    bool hasValue = value.has_value();
    XdrTrait<bool>::serialize(appender, hasValue);
    if (hasValue) {
      XdrTrait<T>::serialize(appender, *value);
    }
  }

  static std::optional<T> deserialize(folly::io::Cursor& cursor) {
    bool hasValue = XdrTrait<bool>::deserialize(cursor);
    if (hasValue) {
      return XdrTrait<T>::deserialize(cursor);
    } else {
      return std::nullopt;
    }
  }

  static size_t serializedSize(const std::optional<T>& value) {
    size_t innerSize = 0;
    if (value.has_value()) {
      innerSize = XdrTrait<T>::serializedSize(*value);
    }
    return XdrTrait<bool>::serializedSize(true) + innerSize;
  }
};

} // namespace facebook::eden

#endif
