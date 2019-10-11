/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/privhelper/UserInfo.h"

#include <folly/Exception.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Stdlib.h>
#include <grp.h>
#include <pwd.h>
#ifdef __linux__
#include <sys/prctl.h>
#endif
#include <vector>
#include "eden/fs/eden-config.h"

#ifdef EDEN_HAVE_SELINUX
#include <selinux/selinux.h> // @manual
#endif // EDEN_HAVE_SELINUX

using folly::checkUnixError;
using folly::throwSystemError;

namespace facebook {
namespace eden {

struct UserInfo::PasswdEntry {
  struct passwd pwd;
  std::vector<char> buf;
};

static void dropToBasicSELinuxPrivileges() {
#ifdef EDEN_HAVE_SELINUX
  const char* baseContext = "user_u:base_r:base_t";

  XLOG(DBG2) << "Dropping SELinux context..." << [&] {
    char* con;
    if (0 == getcon(&con)) {
      SCOPE_SUCCESS {
        freecon(con);
      };
      return " prior context was: " + std::string(con);
    }
    return std::string();
  }();

  // Drop to basic user SELinux privileges.
  // This is required in order to gdb into edenfs without sudo.
  if (setcon(const_cast<char*>(baseContext))) {
    XLOG(DBG3) << "setcon() failed when dropping SELinux context";
  }
#endif // EDEN_HAVE_SELINUX
}

void UserInfo::dropPrivileges() {
  // If we are not privileged, there is nothing to do.
  // Return early in this case; otherwise the initgroups() call below
  // is likely to fail.
  if (uid_ == getuid() && uid_ == geteuid() && gid_ == getgid() &&
      gid_ == getegid()) {
    return;
  }

  // Configure the correct supplementary groups
  auto rc = initgroups(username_.c_str(), gid_);
  checkUnixError(rc, "failed to set supplementary groups");
  // Drop to the correct primary group
  rc = setregid(gid_, gid_);
  checkUnixError(rc, "failed to drop group privileges");
  // Drop to the correct user ID
  rc = setreuid(uid_, uid_);
  checkUnixError(rc, "failed to drop user privileges");

#ifdef __linux__
  // Per PR_SET_DUMPABLE's documentation in ptrace(2), the dumpable bit is set
  // to 0 on any call to setregid or setreuid.  Since we've dropped privileges,
  // reset the dumpable bit to 1 so gdb can attach to Eden without running as
  // root.  This also means that edenfs can produce core dumps.
  rc = prctl(PR_SET_DUMPABLE, 1, 0, 0, 0);
  checkUnixError(rc, "failed to mark process dumpable");
#endif

  // If we started under sudo, update the environment to restore $USER
  // and drop the $SUDO_* variables.
  restoreEnvironmentAfterSudo();

  dropToBasicSELinuxPrivileges();
}

void UserInfo::restoreEnvironmentAfterSudo() {
  // Skip updating the environment if we do not appear to have
  // been started by sudo.
  //
  // Updating the environment is not thread-safe, so let's avoid it if we can.
  // Ideally we should always be dropping privileges before any other threads
  // exist that might be checking environment variables, but it seems better to
  // avoid updating it if possible.
  if (getenv("SUDO_UID") == nullptr) {
    return;
  }

  // Update the $USER environment variable.  This is important so that any
  // subprocesses we spawn (such as "hg debugedenimporthelper") see the correct
  // $USER value.
  setenv("USER", username_.c_str(), 1);
  // sudo also sets the USERNAME and LOGNAME environment variables.
  // Update these as well.
  setenv("USERNAME", username_.c_str(), 1);
  setenv("LOGNAME", username_.c_str(), 1);

  // Clear out the other SUDO_* variables for good measure.
  unsetenv("SUDO_USER");
  unsetenv("SUDO_UID");
  unsetenv("SUDO_GID");
  unsetenv("SUDO_COMMAND");
}

EffectiveUserScope::EffectiveUserScope(const UserInfo& userInfo)
    : ruid_(getuid()), euid_(geteuid()), rgid_(getgid()), egid_(getegid()) {
  checkUnixError(
      setregid(userInfo.getGid(), userInfo.getGid()),
      "setregid() failed in EffectiveUserScope()");
  checkUnixError(
      setreuid(0, userInfo.getUid()),
      "setreuid() failed in EffectiveUserScope()");
}

EffectiveUserScope::~EffectiveUserScope() {
  checkUnixError(
      setreuid(ruid_, euid_), "setreuid() failed in ~EffectiveUserScope()");
  checkUnixError(
      setregid(rgid_, egid_), "setregid() failed in ~EffectiveUserScope()");
}

UserInfo::PasswdEntry UserInfo::getPasswdUid(uid_t uid) {
  static constexpr size_t initialBufSize = 1024;
  static constexpr size_t maxBufSize = 8192;
  PasswdEntry pwd;
  pwd.buf.resize(initialBufSize);

  struct passwd* result;
  while (true) {
    const auto errnum =
        getpwuid_r(uid, &pwd.pwd, pwd.buf.data(), pwd.buf.size(), &result);
    if (errnum == 0) {
      break;
    } else if (errnum == ERANGE && pwd.buf.size() < maxBufSize) {
      // Retry with a bigger buffer
      pwd.buf.resize(pwd.buf.size() * 2);
      continue;
    } else {
      throwSystemError("unable to look up user information for UID ", uid);
    }
  }
  if (result == nullptr) {
    // No user info present for this UID.
    throwSystemError("no passwd entry found for UID ", uid);
  }

  return pwd;
}

bool UserInfo::initFromSudo() {
  // If SUDO_UID is not set, return false indicating we could not
  // find sudo-based identity information.
  const auto sudoUid = getenv("SUDO_UID");
  if (sudoUid == nullptr) {
    return false;
  }

  // Throw an exception if SUDO_GID or SUDI_USER is not set, or if we cannot
  // parse them below.  We want to fail hard if we have SUDO_UID but we can't
  // use it for some reason.  We don't want to fall back to running as root in
  // this case.
  const auto sudoGid = getenv("SUDO_GID");
  if (sudoGid == nullptr) {
    throw std::runtime_error("SUDO_UID set without SUDO_GID");
  }
  const auto sudoUser = getenv("SUDO_USER");
  if (sudoUser == nullptr) {
    throw std::runtime_error("SUDO_UID set without SUDO_USER");
  }

  try {
    uid_ = folly::to<uid_t>(sudoUid);
  } catch (const std::range_error& ex) {
    throw std::runtime_error(
        std::string{"invalid value for SUDO_UID: "} + sudoUid);
  }
  try {
    gid_ = folly::to<gid_t>(sudoGid);
  } catch (const std::range_error& ex) {
    throw std::runtime_error(
        std::string{"invalid value for SUDO_GID: "} + sudoGid);
  }

  username_ = sudoUser;
  initHomedir();
  return true;
}

void UserInfo::initFromNonRoot(uid_t uid) {
  uid_ = uid;
  gid_ = getgid();

  // Always look up the username from the UID.
  // We cannot trust the USER environment variable--the user could have set
  // it to anything.
  auto pwd = getPasswdUid(uid_);
  username_ = pwd.pwd.pw_name;

  initHomedir(&pwd);
}

void UserInfo::initHomedir(PasswdEntry* pwd) {
  // We do trust the $HOME environment variable if it is set.
  // This does not need to be distrusted for security reasons--we can use any
  // arbitrary directory the user wants as long as they have read/write access
  // to it.  We only access it after dropping privileges.
  //
  // Note that we intentionally use canonicalPath() rather than realpath()
  // here.  realpath() will perform symlink resolution.  initHomedir() will
  // generally be run before we have dropped privileges, and we do not want to
  // try traversing symlinks that the user may not actually have permissions to
  // resolve.
  const auto homeEnv = getenv("HOME");
  if (homeEnv != nullptr) {
    homeDirectory_ = canonicalPath(homeEnv);
    return;
  }

  PasswdEntry locallyLookedUp;
  if (!pwd) {
    locallyLookedUp = getPasswdUid(uid_);
    pwd = &locallyLookedUp;
  }

  if (pwd && pwd->pwd.pw_dir) {
    homeDirectory_ = canonicalPath(pwd->pwd.pw_dir);
    return;
  }

  // Fall back to the root directory if all else fails
  homeDirectory_ = AbsolutePath{"/"};
}

UserInfo UserInfo::lookup() {
  UserInfo info;
  // First check the real UID.  If it is non-root, use that.
  // This happens if our binary is setuid root and invoked by a non-root user.
  const auto uid = getuid();
  if (uid != 0) {
    info.initFromNonRoot(uid);
    return info;
  }

  // If we are still here, our real UID is 0.
  // Check the SUDO_* environment variables in case we are running under sudo.
  if (info.initFromSudo()) {
    return info;
  }

  // If we are still here, we are actually running as root and could not find
  // non-root privileges to drop to.
  info.uid_ = uid;
  info.gid_ = getgid();
  auto pwd = getPasswdUid(info.uid_);
  info.username_ = pwd.pwd.pw_name;
  info.initHomedir(&pwd);
  return info;
}
} // namespace eden
} // namespace facebook
