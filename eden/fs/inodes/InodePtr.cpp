/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
template <typename SubclassRawPtrType>
SubclassRawPtrType InodePtr::asSubclass() const {
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

template <typename SubclassPtrType>
SubclassPtrType InodePtr::asSubclassPtr() const {
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

template <typename SubclassPtrType>
SubclassPtrType InodePtr::asSubclassPtrOrNull() const& {
  return SubclassPtrType{
      dynamic_cast<typename SubclassPtrType::InodeType*>(this->value_),
      SubclassPtrType::NORMAL_INCREMENT};
}

template <typename SubclassPtrType>
SubclassPtrType InodePtr::extractSubclassPtr() {
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

template FileInodePtr InodePtr::extractSubclassPtr<FileInodePtr>();
template TreeInodePtr InodePtr::extractSubclassPtr<TreeInodePtr>();

template <typename SubclassPtrType>
SubclassPtrType InodePtr::extractSubclassPtrOrNull() {
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

template FileInodePtr InodePtr::extractSubclassPtrOrNull<FileInodePtr>();
template TreeInodePtr InodePtr::extractSubclassPtrOrNull<TreeInodePtr>();

FileInode* InodePtr::asFile() const {
  return asSubclass<FileInode*>();
}

FileInodePtr InodePtr::asFilePtr() const& {
  return asSubclassPtr<FileInodePtr>();
}

FileInodePtr InodePtr::asFilePtr() && {
  return extractSubclassPtr<FileInodePtr>();
}

FileInode* InodePtr::asFileOrNull() const {
  return dynamic_cast<FileInode*>(this->value_);
}

FileInodePtr InodePtr::asFilePtrOrNull() const& {
  return FileInodePtr{dynamic_cast<FileInode*>(this->value_),
                      FileInodePtr::NORMAL_INCREMENT};
}

FileInodePtr InodePtr::asFilePtrOrNull() && {
  return extractSubclassPtrOrNull<FileInodePtr>();
}

TreeInode* InodePtr::asTree() const {
  return asSubclass<TreeInode*>();
}

TreeInodePtr InodePtr::asTreePtr() const& {
  return asSubclassPtr<TreeInodePtr>();
}

TreeInodePtr InodePtr::asTreePtr() && {
  return extractSubclassPtr<TreeInodePtr>();
}

TreeInode* InodePtr::asTreeOrNull() const {
  return dynamic_cast<TreeInode*>(this->value_);
}

TreeInodePtr InodePtr::asTreePtrOrNull() const& {
  return TreeInodePtr{dynamic_cast<TreeInode*>(this->value_),
                      TreeInodePtr::NORMAL_INCREMENT};
}

TreeInodePtr InodePtr::asTreePtrOrNull() && {
  return extractSubclassPtrOrNull<TreeInodePtr>();
}

// Explicitly instantiate InodePtrImpl for all inode class types
template class InodePtrImpl<FileInode>;
template class InodePtrImpl<TreeInode>;
template FileInodePtr InodePtr::asSubclassPtrOrNull<FileInodePtr>() const&;
template TreeInodePtr InodePtr::asSubclassPtrOrNull<TreeInodePtr>() const&;
} // namespace eden
} // namespace facebook
