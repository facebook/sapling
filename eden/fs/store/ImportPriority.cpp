/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ImportPriority.h"

namespace facebook::eden {

std::string_view ImportPriority::className() const noexcept {
  switch (getClass()) {
    case Class::Low:
      return "Low";
    case Class::Normal:
      return "Normal";
    case Class::High:
      return "High";
  }
  return "Unlabeled";
}

} // namespace facebook::eden
