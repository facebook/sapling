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

#include <type_traits>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {
template <typename InodeType>
template <typename SubclassRawPtrType>
SubclassRawPtrType InodeBasePtrImpl<InodeType>::asSubclass() const {
  if (this->value_ == nullptr) {
    return nullptr;
  }

  auto* subclassPtr = dynamic_cast<SubclassRawPtrType>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(
        std::remove_pointer<SubclassRawPtrType>::type::WRONG_TYPE_ERRNO, *this);
  }
  return subclassPtr;
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::asSubclassPtr() const {
  if (this->value_ == nullptr) {
    return SubclassPtrType{};
  }

  auto* subclassPtr =
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(SubclassPtrType::InodeType::WRONG_TYPE_ERRNO, *this);
  }
  return SubclassPtrType{subclassPtr, SubclassPtrType::NORMAL_INCREMENT};
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::asSubclassPtrOrNull() const& {
  return SubclassPtrType{
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_),
      SubclassPtrType::NORMAL_INCREMENT};
}

template <typename InodeType>
template <typename SubclassPtrType>
SubclassPtrType InodeBasePtrImpl<InodeType>::extractSubclassPtr() {
  if (this->value_ == nullptr) {
    return SubclassPtrType{};
  }

  auto* subclassPtr =
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_);
  if (subclassPtr == nullptr) {
    throw InodeError(SubclassPtrType::InodeType::WRONG_TYPE_ERRNO, *this);
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
  return asSubclass<FileInodeRawPtr>();
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtr() const& {
  return asSubclassPtr<FileInodePtr>();
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::FileInodePtr
InodeBasePtrImpl<InodeType>::asFilePtr()&& {
  return extractSubclassPtr<FileInodePtr>();
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
  return asSubclass<TreeInodeRawPtr>();
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtr() const& {
  return asSubclassPtr<TreeInodePtr>();
}

template <typename InodeType>
typename detail::InodePtrTraits<InodeType>::TreeInodePtr
InodeBasePtrImpl<InodeType>::asTreePtr()&& {
  return extractSubclassPtr<TreeInodePtr>();
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
template FileInodePtr
InodeBasePtrImpl<InodeBase>::asSubclassPtrOrNull<FileInodePtr>() const&;
template TreeInodePtr
InodeBasePtrImpl<InodeBase>::asSubclassPtrOrNull<TreeInodePtr>() const&;
}
}
