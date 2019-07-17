// Exporting a small subset of libproc (macOS-specific) features.
//
// Ideally the "libproc" crate can be used. At the time of writing,
// libproc does not expose the proc_bsdshortinfo struct, therefore
// cannot provide the "parent process" information.
//
// See:
// https://github.com/osquery/osquery/blob/4.0.0/osquery/tables/system/darwin/processes.cpp

#include <assert.h>
#include <libproc.h>
#include <mach-o/dyld_images.h>
#include <mach/mach.h>

/// Return pid's parent process id.
/// Return 0 on error or if pid does not have a parent.
pid_t darwin_ppid(pid_t pid) {
  struct proc_bsdshortinfo proc;
  proc.pbsi_ppid = 0;
  if (proc_pidinfo(
          pid, PROC_PIDT_SHORTBSDINFO, 1, &proc, PROC_PIDT_SHORTBSDINFO_SIZE) ==
      PROC_PIDT_SHORTBSDINFO_SIZE) {
    return proc.pbsi_ppid;
  }
  return 0;
}

/// Return the executable path. Not thread-safe. Not reentrant.
const char* darwin_exepath(pid_t pid) {
  static char path[PROC_PIDPATHINFO_MAXSIZE + 1];
  int len = proc_pidpath(pid, path, PROC_PIDPATHINFO_MAXSIZE);
  if (len <= 0) {
    path[0] = 0;
  } else {
    assert(len < (int)sizeof(path));
    path[len] = 0;
  }
  return path;
}
