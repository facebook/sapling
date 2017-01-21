/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/InodePtr-defs.h"

#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {
template <typename InodeType>
template <typename SubclassRawPtrType>
SubclassRawPtrType InodeBasePtrImpl<InodeType>::asSubclass(
    int errnoValue) const {
  if (this->value_ == nullptr) {
    return nullptr;
  }

  auto* subclassPtr = dynamic_cast<SubclassRawPtrType>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(errnoValue, *this);
  }
  return subclassPtr;
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::asSubclassPtr(
    int errnoValue) const {
  if (this->value_ == nullptr) {
    return SubclassPtrType{};
  }

  auto* subclassPtr =
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(errnoValue, *this);
  }
  return SubclassPtrType{subclassPtr, SubclassPtrType::NORMAL_INCREMENT};
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::extractSubclassPtr(
    int errnoValue) {
  if (this->value_ == nullptr) {
    return SubclassPtrType{};
  }

  auto* subclassPtr =
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(errnoValue, *this);
  }
  this->value_ = nullptr;
  return SubclassPtrType{subclassPtr, SubclassPtrType::NO_INCREMENT};
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::extractSubclassPtrOrNull() {
  if (this->value_ == nullptr) {
    return SubclassPtrType{};
  }
  auto* subclassPtr =
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_);
  if (subclassPtr == nullptr) {
    return SubclassPtrType{};
  }
  this->value_ = nullptr;
  return SubclassPtrType{subclassPtr, SubclassPtrType::NO_INCREMENT};
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInode*
InodeBasePtrImpl<InodeType>::asFile() const {
  return asSubclass<FileInodeRawPtr>(EISDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtr() const& {
  return asSubclassPtr<FileInodePtr>(EISDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtr()&& {
  return extractSubclassPtr<FileInodePtr>(EISDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInode*
InodeBasePtrImpl<InodeType>::asFileOrNull() const {
  return dynamic_cast<FileInodeRawPtr>(this->value_);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtrOrNull() const& {
  return FileInodePtr{dynamic_cast<FileInodeRawPtr>(this->value_),
                      FileInodePtr::NORMAL_INCREMENT};
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtrOrNull()&& {
  return extractSubclassPtrOrNull<FileInodePtr>();
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInode*
InodeBasePtrImpl<InodeType>::asTree() const {
  return asSubclass<TreeInodeRawPtr>(ENOTDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtr() const& {
  return asSubclassPtr<TreeInodePtr>(ENOTDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtr()&& {
  return extractSubclassPtr<TreeInodePtr>(ENOTDIR);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInode*
InodeBasePtrImpl<InodeType>::asTreeOrNull() const {
  return dynamic_cast<TreeInodeRawPtr>(this->value_);
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtrOrNull() const& {
  return TreeInodePtr{dynamic_cast<TreeInodeRawPtr>(this->value_),
                      TreeInodePtr::NORMAL_INCREMENT};
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtrOrNull()&& {
  return extractSubclassPtrOrNull<TreeInodePtr>();
}

// Explicitly instantiate InodePtrImpl for all inode class types
template class InodeBasePtrImpl<InodeBase>;
template class InodePtrImpl<FileInode>;
template class InodePtrImpl<TreeInode>;
}
}
