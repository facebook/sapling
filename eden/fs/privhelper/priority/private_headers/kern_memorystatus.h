/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __APPLE__

/**
 * NOTE: These structures/constants are copied from private macOS kernel
 * headers. To be exact, these values were taken from the latest XNU release at
 * the time, xnu-11417.101.15:
 * https://github.com/apple-oss-distributions/xnu/blob/e3723e1f17661b24996789d8afc084c0c3303b26/bsd/sys/kern_memorystatus.h
 *
 * These structures may need to be updated in the future. Monitor this repo for
 * future updates: https://github.com/apple-oss-distributions/xnu/
 */

#ifndef SYS_MEMORYSTATUS_H
#define SYS_MEMORYSTATUS_H

#include <sys/proc.h> // @manual
#include <sys/time.h> // @manual

/* Method signatures for private macOS syscalls */
extern "C" int memorystatus_get_level(user_addr_t level);
extern "C" int memorystatus_control(
    uint32_t command,
    int32_t pid,
    uint32_t flags,
    void* buffer,
    size_t buffersize);

/* Structures for memorystatus_get_level */
typedef uint32_t memorystatus_proc_state_t;

typedef struct memorystatus_priority_entry {
  pid_t pid;
  int32_t priority;
  uint64_t user_data;
  int32_t limit; /* MB */
  memorystatus_proc_state_t state;
} memorystatus_priority_entry_t;

/* Structures for memorystatus_control */
#define MEMORYSTATUS_MPE_VERSION_1 1

#define MEMORYSTATUS_MPE_VERSION_1_SIZE \
  sizeof(struct memorystatus_properties_entry_v1)

typedef struct memorystatus_properties_entry_v1 {
  int version;
  pid_t pid;
  int32_t priority;
  int use_probability;
  uint64_t user_data;
  int32_t limit; /* MB */
  uint32_t state;
  char proc_name[MAXCOMLEN + 1];
  char __pad1[3];
} memorystatus_properties_entry_v1_t;

typedef struct memorystatus_priority_properties {
  int32_t priority;
  uint64_t user_data;
} memorystatus_priority_properties_t;

/* Magic numbers for invoking the memorystatus_control() method */
#define MEMORYSTATUS_CMD_GET_PRIORITY_LIST 1
#define MEMORYSTATUS_SET_PRIORITY_ASSERTION 0x1
#define MEMORYSTATUS_CMD_GRP_SET_PROPERTIES 100
#define MEMORYSTATUS_FLAGS_GRP_SET_PRIORITY 0x8

/* Jetsam priority levels */
#define JETSAM_PRIORITY_IDLE 0
#define JETSAM_PRIORITY_REVISION 2
#define JETSAM_PRIORITY_DEFAULT 180
#define JETSAM_PRIORITY_IMPORTANT 180
#define JETSAM_PRIORITY_CRITICAL 190
#define JETSAM_PRIORITY_MAX 210

#endif /* SYS_MEMORYSTATUS_H */

#endif /* __APPLE__ */
