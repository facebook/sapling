/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <folly/io/Cursor.h>
#include <variant>

namespace facebook::eden {

/**
 * Trait used to XDR encode a type.
 *
 * A struct that needs serializing will need to implement the 2 functions:
 *
 * `static void serialize(folly::io::Appender& appender, const T& value)`
 * `static T deserialize(folly::io::Cursor& cursor)`
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
  static void serialize(folly::io::Appender& appender, T value) {
    appender.writeBE<T>(value);
  }

  static T deserialize(folly::io::Cursor& cursor) {
    return cursor.readBE<T>();
  }
};

/**
 * Boolean values are encoded as a 0/1 integer.
 */
template <>
struct XdrTrait<bool> {
  static void serialize(folly::io::Appender& appender, bool value) {
    XdrTrait<int32_t>::serialize(appender, value ? 1 : 0);
  }

  static bool deserialize(folly::io::Cursor& cursor) {
    return XdrTrait<int32_t>::deserialize(cursor) ? true : false;
  }
};

/**
 * Enumeration values are encoded as a integer.
 */
template <typename T>
struct XdrTrait<T, typename std::enable_if_t<std::is_enum_v<T>>> {
  static void serialize(folly::io::Appender& appender, const T& value) {
    XdrTrait<int32_t>::serialize(appender, static_cast<int32_t>(value));
  }

  static T deserialize(folly::io::Cursor& cursor) {
    return static_cast<T>(XdrTrait<int32_t>::deserialize(cursor));
  }
};

namespace detail {

/**
 * XDR arrays are 4-bytes aligned, make sure we write and skip these when
 * serializing/deserializing data.
 */
inline size_t roundUp(size_t value) {
  return (value + 3) & ~3;
}

/**
 * Serialize a fixed size byte array. Their content is serialized as is, padded
 * with NUL bytes to align on a 4-byte boundary.
 */
void serialize_fixed(folly::io::Appender& appender, folly::ByteRange value);

/**
 * Serialize a variable size byte array. The size of the array is written
 * first, followed by the content of the array, this is also aligned on a
 * 4-byte boundary.
 */
void serialize_variable(folly::io::Appender& appender, folly::ByteRange value);

/**
 * Serialize an IOBuf chain. This is serialized like a variable sized array,
 * ie: size first, followed by the content and aligned on a 4-byte boundary.
 */
void serialize_iobuf(folly::io::Appender& appender, const folly::IOBuf& buf);

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
      folly::io::Appender& appender,
      const std::array<uint8_t, N>& value) {
    detail::serialize_fixed(appender, folly::ByteRange(value));
  }

  static std::array<uint8_t, N> deserialize(folly::io::Cursor& cursor) {
    std::array<uint8_t, N> ret;
    cursor.pull(ret.data(), N);
    detail::skipPadding(cursor, N);
    return ret;
  }
};

template <typename T, size_t N>
struct XdrTrait<
    std::array<T, N>,
    typename std::enable_if_t<!std::is_same_v<T, uint8_t>>> {
  static void serialize(
      folly::io::Appender& appender,
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
};

/**
 * Vectors are encoded as a variable sized array: length, followed by its
 * content.
 */
template <>
struct XdrTrait<std::vector<uint8_t>> {
  static void serialize(
      folly::io::Appender& appender,
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
};

/**
 * IOBuf are encoded as a variable sized array, similarly to a vector. IOBuf
 * should be preferred to a vector when the data to serialize/deserialize is
 * potentially large, a vector would copy all the data, while an IOBuf would
 * clone the existing cursor.
 *
 * TODO(xavierd): folly::io::Appender doesn't have a way to zero-copy append to
 * it, maybe a folly::io::QueueAppender would be better fit than
 * folly::io::Appender?
 */
template <>
struct XdrTrait<std::unique_ptr<folly::IOBuf>> {
  static void serialize(
      folly::io::Appender& appender,
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
};

template <typename T>
struct XdrTrait<
    std::vector<T>,
    typename std::enable_if_t<!std::is_same_v<T, uint8_t>>> {
  static void serialize(
      folly::io::Appender& appender,
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
};

/**
 * Strings are encoded in the same way as a vector.
 */
template <>
struct XdrTrait<std::string> {
  static void serialize(
      folly::io::Appender& appender,
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
      folly::io::Appender& appender,
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
  /* implicit */ XdrOptionalVariant(TrueVariant&& set)
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

} // namespace facebook::eden

#endif
