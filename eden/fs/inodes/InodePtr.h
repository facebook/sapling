/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "InodePtrFwd.h"

#include <glog/logging.h>
#include <cstddef>
#include <memory>
#include <utility>

namespace facebook {
namespace eden {

/**
 * A custom smart pointer class for pointing to Inode objects.
 *
 * This maintains a reference count similar to std::shared_ptr.  However, we do
 * slightly more custom management of Inode ownership.  Inodes are not
 * immediately destroyed when their reference count drops to 0.  Instead they
 * simply become available to unload.
 *
 * InodePtrImpl is used to implement FileInodePtr and TreeInodePtr.
 * For InodeBase, see the InodePtr class, which derives from
 * InodePtrImpl<InodeBase>.
 */
template <typename InodeTypeParam>
class InodePtrImpl {
 public:
  using InodeType = InodeTypeParam;

  /**
   * The default constructor null-initializes the pointer.
   */
  constexpr InodePtrImpl() noexcept {}

  /**
   * Implicit construction from nullptr
   */
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
   * Like newPtrLocked() but consumes the given unique_ptr.
   */
  static InodePtrImpl takeOwnership(std::unique_ptr<InodeType> value) noexcept {
    return InodePtrImpl{value.release(), LOCKED_INCREMENT};
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
  friend class InodePtr;

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

/**
 * An InodePtrImpl pointing to InodeBase.
 *
 * This derives from InodePtrImpl<InodeBase>, and adds a few methods for
 * converting to specific Inode subclasses.
 */
class InodePtr : public InodePtrImpl<InodeBase> {
 public:
  /**
   * The default constructor null-initializes the pointer.
   */
  constexpr InodePtr() noexcept {}

  /**
   * Implicit construction from nullptr
   */
  constexpr /* implicit */ InodePtr(std::nullptr_t) noexcept {}

  /*
   * Templated constructors and assignment operators.
   *
   * These support construction of InodePtr from pointers to InodeBase
   * subclasses (e.g., FileInodePtr and TreeInodePtr).
   */

  template <typename Inode>
  /* implicit */ InodePtr(const InodePtrImpl<Inode>& other) noexcept
      : InodePtrImpl<InodeBase>{other.value_, NORMAL_INCREMENT} {}

  template <typename Inode>
  /* implicit */ InodePtr(InodePtrImpl<Inode>&& other) noexcept
      : InodePtrImpl<InodeBase>{other.value_, NO_INCREMENT} {
    other.value_ = nullptr;
  }

  template <typename Inode>
  InodePtr& operator=(const InodePtrImpl<Inode>& other) noexcept {
    if (value_ != other.value_) {
      decref();
      value_ = other.value_;
      incref();
    }
    return *this;
  }

  template <typename Inode>
  InodePtr& operator=(InodePtrImpl<Inode>&& other) noexcept {
    decref();
    value_ = other.value_;
    other.value_ = nullptr;
    return *this;
  }

  /*
   * Override newPtrLocked(), takeOwnership(), and newPtrFromExisting() to
   * return an InodePtr instead of the InodePtrImpl parent class.
   */
  static InodePtr newPtrLocked(InodeBase* value) noexcept {
    return InodePtr{value, InodePtrImpl<InodeBase>::LOCKED_INCREMENT};
  }
  static InodePtr takeOwnership(std::unique_ptr<InodeBase> value) noexcept {
    return InodePtr::newPtrLocked(value.release());
  }
  static InodePtr newPtrFromExisting(InodeBase* value) noexcept {
    return InodePtr{value, InodePtrImpl<InodeBase>::NORMAL_INCREMENT};
  }

  /**
   * Convert this InodePtr to a FileInodePtr.
   *
   * Throws EISDIR if this points to a TreeInode instead of a FileInode.
   * Returns a null FileInodePtr if this pointer is null.
   */
  FileInode* asFile() const;
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
  FileInode* asFileOrNull() const;
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
  TreeInode* asTree() const;
  TreeInodePtr asTreePtr() const&;
  TreeInodePtr asTreePtr() &&;

  /**
   * Convert this InodePtr to a TreeInodePtr.
   *
   * Returns a null pointer if this points to a FileInode instead of a
   * TreeInode.
   */
  TreeInode* asTreeOrNull() const;
  TreeInodePtr asTreePtrOrNull() const&;
  TreeInodePtr asTreePtrOrNull() &&;

  template <typename SubclassPtrType>
  SubclassPtrType asSubclassPtrOrNull() const&;
  template <typename SubclassPtrType>
  SubclassPtrType asSubclassPtrOrNull() && {
    return extractSubclassPtrOrNull<SubclassPtrType>();
  }

 private:
  // Privately inherit our parent class's other protected constructors
  using InodePtrImpl<InodeBase>::InodePtrImpl;

  template <typename SubclassRawPtrType>
  SubclassRawPtrType asSubclass() const;
  template <typename SubclassPtrType>
  SubclassPtrType asSubclassPtr() const;
  template <typename SubclassPtrType>
  SubclassPtrType extractSubclassPtr();
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
} // namespace eden
} // namespace facebook
