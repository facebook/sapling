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

/*
 * This file contains definitions of a few simple and commonly called
 * InodePtrImpl methods.  It is useful for callers to see the definition of
 * these methods so they can be inlined.
 *
 * This file is included automatically by InodeBase.h
 */

#include "InodePtr.h"

#include "InodeBase.h"

namespace facebook {
namespace eden {

template <typename InodeType>
InodePtrImpl<InodeType>::InodePtrImpl(
    InodeType* value,
    NormalIncrementEnum) noexcept
    : value_(value) {
  if (value_) {
    value_->incrementPtrRef();
  }
}

template <typename InodeType>
InodePtrImpl<InodeType>::InodePtrImpl(
    InodeType* value,
    LockedIncrementEnum) noexcept
    : value_(value) {
  // We don't check for value_ == nullptr here.
  // The caller should always ensure the argument is non-null for this call.
  value_->newInodeRefConstructed();
}

template <typename InodeType>
void InodePtrImpl<InodeType>::incref() {
  if (value_) {
    value_->incrementPtrRef();
  }
}

template <typename InodeType>
void InodePtrImpl<InodeType>::decref() {
  if (value_) {
    value_->decrementPtrRef();
  }
}

template <typename InodeType>
void InodePtrImpl<InodeType>::manualDecRef() {
  CHECK_NOTNULL(value_);
  value_->decrementPtrRef();
}

template <typename InodeType>
void InodePtrImpl<InodeType>::resetNoDecRef() {
  CHECK_NOTNULL(value_);
  value_ = nullptr;
}
}
}
