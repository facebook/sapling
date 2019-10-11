/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <dirent.h>
#include <sys/types.h>
#include <cstdint>

namespace facebook {
namespace eden {

/** Represents the type of a filesystem entry.
 *
 * This is the same type and intent as the d_type field of a dirent struct.
 *
 * We provide an explicit type to make it clearer when we're working
 * with this value.
 *
 * https://www.daemon-systems.org/man/DTTOIF.3.html
 *
 * Portability note: Solaris does not have a d_type field, so this
 * won't compile.  We don't currently have plans to support Solaris.
 */
enum class dtype_t : decltype(dirent::d_type) {
  Unknown = DT_UNKNOWN,
  Fifo = DT_FIFO,
  Char = DT_CHR,
  Dir = DT_DIR,
  Block = DT_BLK,
  Regular = DT_REG,
  Symlink = DT_LNK,
  Socket = DT_SOCK,
  Whiteout = DT_WHT,
};

/// Convert to a form suitable for inserting into a stat::st_mode
inline mode_t dtype_to_mode(dtype_t dt) {
  return DTTOIF(static_cast<uint8_t>(dt));
}

/// Convert from stat::st_mode form to dirent::d_type form
inline dtype_t mode_to_dtype(mode_t mode) {
  return static_cast<dtype_t>(IFTODT(mode));
}
} // namespace eden
} // namespace facebook
