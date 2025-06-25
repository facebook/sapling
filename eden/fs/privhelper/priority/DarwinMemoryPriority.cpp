/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __APPLE__

#include "eden/fs/privhelper/priority/DarwinMemoryPriority.h"

#include <errno.h> // @manual
#include <folly/logging/xlog.h>

#include "eden/common/utils/Throw.h"
#include "eden/fs/privhelper/priority/private_headers/kern_memorystatus.h"

namespace facebook::eden {

DarwinMemoryPriority::DarwinMemoryPriority(int32_t jetsam_priority)
    : MemoryPriority(jetsam_priority) {
  // Jetsam priorities range from 0 to 210, with 0 being the most likely to be
  // killed, and 210 being very unlikely to be killed.
  //
  // https://www.newosxbook.com/articles/MemoryPressure.html
  if (jetsam_priority < JETSAM_PRIORITY_IDLE ||
      jetsam_priority > JETSAM_PRIORITY_MAX) {
    throwf<std::invalid_argument>(
        "Invalid Jetsam priority: {}. Must be between {} and {} inclusive.",
        jetsam_priority,
        JETSAM_PRIORITY_IDLE,
        JETSAM_PRIORITY_MAX);
  }

  // The current default priority is 180, which means setting a priority lower
  // than that makes EdenFS more likely to be killed.
  if (jetsam_priority < JETSAM_PRIORITY_DEFAULT) {
    XLOGF(
        WARN,
        "Setting a Jetsam priority below {} is not recommended. Priority: {}",
        JETSAM_PRIORITY_DEFAULT,
        jetsam_priority);
  }
  priority_ = jetsam_priority;
}

int DarwinMemoryPriority::setPriorityForProcess(pid_t pid) {
  if (__builtin_available(macOS 10.10, iOS 8.0, tvOS 9.0, watchOS 1.0, *)) {
    memorystatus_properties_entry_v1_t properties;
    memset(&properties, 0, sizeof(memorystatus_properties_entry_v1_t));
    properties.pid = pid;
    properties.priority = priority_;
    properties.version = MEMORYSTATUS_MPE_VERSION_1;

    if (memorystatus_control(
            MEMORYSTATUS_CMD_GRP_SET_PROPERTIES,
            0,
            MEMORYSTATUS_FLAGS_GRP_SET_PRIORITY,
            &properties,
            sizeof(memorystatus_properties_entry_v1_t)) == -1) {
      XLOGF(
          ERR,
          "memorystatus_control(MEMORYSTATUS_CMD_GRP_SET_PROPERTIES) error: {}: {}",
          errno,
          strerror(errno));
      return errno;
    }
  } else {
    XLOGF(ERR, "Setting priority is not supported on this OS");
    return ENOTSUP;
  }
  XLOGF(
      INFO,
      "The priority of pid {} was set to {} successfully.",
      pid,
      priority_);

  return 0;
}

std::optional<int32_t> DarwinMemoryPriority::getPriorityForProcess(pid_t pid) {
  memorystatus_priority_entry_t prio_entry;
  if (__builtin_available(macOS 10.9, iOS 7.0, tvOS 9.0, watchOS 1.0, *)) {
    if (memorystatus_control(
            MEMORYSTATUS_CMD_GET_PRIORITY_LIST,
            pid,
            0,
            &prio_entry,
            sizeof(prio_entry)) == -1) {
      XLOGF(
          ERR,
          "memorystatus_control(MEMORYSTATUS_CMD_GET_PRIORITY_LIST) error: {}: {}",
          errno,
          strerror(errno));
      return std::nullopt;
    }
  } else {
    XLOGF(ERR, "Getting priority is not supported on this OS");
    return std::nullopt;
  }
  XLOGF(DBG3, "Priority of pid {}: {}\n", pid, prio_entry.priority);
  return prio_entry.priority;
}
} // namespace facebook::eden

#endif // __APPLE__
