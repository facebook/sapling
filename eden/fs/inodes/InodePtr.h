/*
 *  Copyright (c) 2017, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "InodePtrFwd.h"

#include <glog/logging.h>
#include <cstddef>
#include <utility>

namespace facebook {
namespace eden {

/**
 * A custom smart pointer class for pointing to Inode objects.
 *
 * This maintains a reference count similar to std::shared_ptr.  However, we do
 * slightly more custom management of Inode ownership.  Inodes are not
 * immediately destroyed when their reference count drops to 0.  Instead they
 * simply become available to unload
 */
template <typename InodeTypeParam>
class InodePtrImpl {
 public:
  using InodeType = InodeTypeParam;

  constexpr InodePtrImpl() noexcept {}
  constexpr /* implicit */ InodePtrImpl(std::nullptr_t) noexcept {}
  ~InodePtrImpl() {
    decref();
  }

  /*
   * Copy/move constructors and assignment operators
   */
  InodePtrImpl(const InodePtrImpl& other) noexcept : value_(other.value_) {
    incref();
  }
  InodePtrImpl(InodePtrImpl&& other) noexcept {
    value_ = other.value_;
    other.value_ = nullptr;
  }
  InodePtrImpl& operator=(const InodePtrImpl& other) noexcept {
    // Only update reference counts if we are not already pointing at the
    // desired Inode.
    //
    // This handles self assignment, and is necessary to ensure that we do not
    // decrement our Inode's reference count to 0 in the middle of self
    // assignment.
    if (value_ != other.value_) {
      decref();
      value_ = other.value_;
      incref();
    }

    return *this;
  }
  InodePtrImpl& operator=(InodePtrImpl&& other) noexcept {
    // The C++ standard says that move self-assignment is undefined.
    // http://www.open-std.org/jtc1/sc22/wg21/docs/lwg-defects.html#1204
    //
    // Make sure our callers never try to move assign an InodePtr from itself.
    DCHECK_NE(this, &other);

    decref();
    value_ = other.value_;
    other.value_ = nullptr;
    return *this;
  }

  /*
   * Templated constructors and assignment operators.
   *
   * These support:
   * - construction of InodePtr<InodeBase> from inode subclasses like
   *   InodePtr<FileInode>
   * - construction of InodePtr<InodeType> from InodePtr<const InodeType>
   */

  template <typename Inode>
  /* implicit */ InodePtrImpl(const InodePtrImpl<Inode>& other) noexcept
      : value_(other.value_) {
    incref();
  }

  template <typename Inode>
  /* implicit */ InodePtrImpl(InodePtrImpl<Inode>&& other) noexcept {
    value_ = other.value_;
    other.value_ = nullptr;
  }

  template <typename Inode>
  InodePtrImpl& operator=(const InodePtrImpl<Inode>& other) noexcept {
    if (value_ != other.value_) {
      decref();
      value_ = other.value_;
      incref();
    }
    return *this;
  }

  template <typename Inode>
  InodePtrImpl& operator=(InodePtrImpl<Inode>&& other) noexcept {
    DCHECK_NE(this, &other);
    decref();
    value_ = other.value_;
    other.value_ = nullptr;
    return *this;
  }

  /**
   * Explicit boolean conversion.
   *
   * Returns !isNull()
   */
  explicit operator bool() const {
    return value_ != nullptr;
  }

  InodeType* get() const {
    return value_;
  }
  InodeType* operator->() const {
    return value_;
  }
  InodeType& operator*() const {
    return *value_;
  }

  void reset() {
    decref();
    value_ = nullptr;
  }

  /**
   * An API for InodeMap::lookupInode() and TreeInode::getOrLoadChild() to use
   * when constructing new InodePtr objects to return to callers.
   *
   * This API should only be used by these two call sites.  All other callers
   * should use one of those two APIs (or their related helper functions) to
   * obtain InodePtrs
   *
   * This API should only be called when holding the InodeMap lock, or the
   * parent TreeInode's contents lock.
   */
  static InodePtrImpl newPtrLocked(InodeType* value) noexcept {
    return InodePtrImpl{value, LOCKED_INCREMENT};
  }

  /**
   * An API for TreeInode to use to construct an InodePtr from itself in order
   * to give to new children inodes that it creates.
   *
   * It should always be the case that the caller (the one asking the TreeInode
   * for its child) already has an existing reference to the TreeInode.
   * Therefore the TreeInode's refcount should already be at least 1, and this
   * API should never cause a refcount transition from 0 to 1.
   */
  static InodePtrImpl newPtrFromExisting(InodeType* value) noexcept {
    return InodePtrImpl{value, NORMAL_INCREMENT};
  }

  template <typename... Args>
  static InodePtrImpl makeNew(Args&&... args) {
    auto* inode = new InodeType(std::forward<Args>(args)...);
    return InodePtrImpl{inode, LOCKED_INCREMENT};
  }

  /**
   * manualDecRef() is an internal method only for use by InodeMap.
   *
   * InodeMap will call this on the root inode to manually release its
   * reference count when the mount point starts shutting down.  It will then
   * call resetNoDecRef() once the root inode becomes fully unreferenced.
   */
  void manualDecRef();

  /**
   * resetNoDecRef() is an internal method only for use by InodeMap.
   *
   * InodeMap will call this on its root inode after having manually released
   * the reference count with manualDecRef().
   */
  void resetNoDecRef();

 protected:
  template <typename OtherInodeType>
  friend class InodePtrImpl;
  template <typename OtherInodeType>
  friend class InodeBasePtrImpl;

  enum NoIncrementEnum { NO_INCREMENT };
  enum NormalIncrementEnum { NORMAL_INCREMENT };
  enum LockedIncrementEnum { LOCKED_INCREMENT };

  // Protected constructors for internal use.
  InodePtrImpl(InodeType* value, NormalIncrementEnum) noexcept;
  InodePtrImpl(InodeType* value, LockedIncrementEnum) noexcept;
  InodePtrImpl(InodeType* value, NoIncrementEnum) noexcept : value_(value) {}

  void incref();
  void decref();

  InodeType* value_{nullptr};
};

namespace detail {
// Helper class so that InodePtr and InodeBasePtr can return
// FileInodePtr vs ConstFileInodePtr appropriately, and similarly for TreeInode
// pointers.
template <typename BaseType>
struct InodePtrTraits;
template <>
struct InodePtrTraits<InodeBase> {
  using FileInode = ::facebook::eden::FileInode;
  using FileInodePtr = ::facebook::eden::FileInodePtr;
  using TreeInode = ::facebook::eden::TreeInode;
  using TreeInodePtr = ::facebook::eden::TreeInodePtr;
};

template <>
struct InodePtrTraits<const InodeBase> {
  using FileInode = const ::facebook::eden::FileInode;
  using FileInodePtr = ConstFileInodePtr;
  using TreeInode = const ::facebook::eden::TreeInode;
  using TreeInodePtr = ConstTreeInodePtr;
};
}

/**
 * An InodePtr pointing to InodeBase.
 *
 * This derives from the generic InodePtrImpl class, and adds a few methods for
 * converting to specific Inode subclasses.
 */
template <typename InodeTypeParam>
class InodeBasePtrImpl : public InodePtrImpl<InodeTypeParam> {
 public:
  using InodeType = InodeTypeParam;
  using FileInodeRawPtr =
      typename detail::InodePtrTraits<InodeTypeParam>::FileInode*;
  using FileInodePtr =
      typename detail::InodePtrTraits<InodeTypeParam>::FileInodePtr;
  using TreeInodeRawPtr =
      typename detail::InodePtrTraits<InodeTypeParam>::TreeInode*;
  using TreeInodePtr =
      typename detail::InodePtrTraits<InodeTypeParam>::TreeInodePtr;

  /* Inherit all of our parent class's constructors */
  using InodePtrImpl<InodeType>::InodePtrImpl;

  /**
   * Convert this InodePtr to a FileInodePtr.
   *
   * Throws EISDIR if this points to a TreeInode instead of a FileInode.
   * Returns a null FileInodePtr if this pointer is null.
   */
  FileInodeRawPtr asFile() const;
  FileInodePtr asFilePtr() const&;
  /**
   * Extract the pointer from this InodePtr and put it in a FileInodePtr
   * object.  This  InodePtr is reset to null.
   *
   * Throws EISDIR if this points to a TreeInode instead of a FileInode.
   * On error this InodePtr object will be left unchanged (it is not reset to
   * null).
   */
  FileInodePtr asFilePtr() &&;

  /**
   * Convert this InodePtr to a FileInodePtr.
   *
   * Returns a null pointer if this points to a TreeInode instead of a
   * FileInode.
   */
  FileInodeRawPtr asFileOrNull() const;
  FileInodePtr asFilePtrOrNull() const&;
  /**
   * Extract the pointer from this InodePtr and put it in a FileInodePtr
   * object.  This  InodePtr is reset to null.
   *
   * Returns null if this points to a TreeInode instead of a FileInode.
   * On error this InodePtr object will be left unchanged (it is not reset to
   * null).
   */
  FileInodePtr asFilePtrOrNull() &&;

  /**
   * Convert this InodePtr to a TreeInodePtr.
   *
   * Throws ENOTDIR if this points to a FileInode instead of a TreeInode.
   * Returns a null TreeInodePtr if this pointer is null.
   */
  TreeInodeRawPtr asTree() const;
  TreeInodePtr asTreePtr() const&;
  TreeInodePtr asTreePtr() &&;

  /**
   * Convert this InodePtr to a TreeInodePtr.
   *
   * Returns a null pointer if this points to a FileInode instead of a
   * TreeInode.
   */
  TreeInodeRawPtr asTreeOrNull() const;
  TreeInodePtr asTreePtrOrNull() const&;
  TreeInodePtr asTreePtrOrNull() &&;

 private:
  template <typename SubclassRawPtrType>
  SubclassRawPtrType asSubclass(int errnoValue) const;
  template <typename SubclassPtrType>
  SubclassPtrType asSubclassPtr(int errnoValue) const;
  template <typename SubclassPtrType>
  SubclassPtrType extractSubclassPtr(int errnoValue);
  template <typename SubclassPtrType>
  SubclassPtrType extractSubclassPtrOrNull();
};

/*
 * Operators to compare InodePtr types
 */
template <typename T, typename U>
bool operator==(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() == b.get();
}
template <typename T, typename U>
bool operator!=(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() != b.get();
}
template <typename T, typename U>
bool operator<(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() < b.get();
}
template <typename T, typename U>
bool operator<=(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() <= b.get();
}
template <typename T, typename U>
bool operator>(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() > b.get();
}
template <typename T, typename U>
bool operator>=(const InodePtrImpl<T>& a, const InodePtrImpl<U>& b) {
  return a.get() >= b.get();
}

/*
 * Operators to compare InodePtrImpl with nullptr
 */
template <typename InodeTypeParam>
bool operator==(const InodePtrImpl<InodeTypeParam>& ptr, std::nullptr_t) {
  return !bool(ptr);
}
template <typename InodeTypeParam>
bool operator!=(const InodePtrImpl<InodeTypeParam>& ptr, std::nullptr_t) {
  return bool(ptr);
}
template <typename InodeTypeParam>
bool operator==(std::nullptr_t, const InodePtrImpl<InodeTypeParam>& ptr) {
  return !bool(ptr);
}
template <typename InodeTypeParam>
bool operator!=(std::nullptr_t, const InodePtrImpl<InodeTypeParam>& ptr) {
  return bool(ptr);
}
}
}
