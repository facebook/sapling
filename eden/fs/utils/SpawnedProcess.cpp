/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/SpawnedProcess.h"
#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/ScopeGuard.h>
#include <folly/String.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventBaseManager.h>
#include <folly/logging/xlog.h>
#include <folly/system/Shell.h>
#include <signal.h>
#include <chrono>
#include <memory>
#include <system_error>
#include <thread>

#ifndef _WIN32
#include <folly/portability/Unistd.h>
#include <sys/poll.h>
#include <sys/wait.h>
#else
#include "eden/common/utils/StringConv.h"
#endif

using folly::checkPosixError;
using namespace std::chrono_literals;

#ifndef _WIN32
// POSIX doesn't appear to specify which header defines this,
// so we just extern it.
extern "C" {
extern char** environ;
}
#endif

namespace facebook::eden {

ProcessStatus ProcessStatus::fromWaitStatus(int rawStatus) {
#ifndef _WIN32
  if (WIFEXITED(rawStatus)) {
    return ProcessStatus(ProcessStatus::State::Exited, rawStatus);
  }
  if (WIFSIGNALED(rawStatus)) {
    return ProcessStatus(ProcessStatus::State::Killed, rawStatus);
  }
#endif
  return ProcessStatus(ProcessStatus::State::Exited, rawStatus);
}

int ProcessStatus::exitStatus() const {
  if (state_ == State::Exited) {
#ifndef _WIN32
    return WEXITSTATUS(status_);
#else
    return status_;
#endif
  }
  return 1;
}

int ProcessStatus::killSignal() const {
#ifndef _WIN32
  if (state_ == State::Killed) {
    return WTERMSIG(status_);
  }
#endif
  return 0;
}

std::string ProcessStatus::str() const {
  switch (state_) {
    case ProcessStatus::State::NotStarted:
      return "not started";
    case ProcessStatus::State::Running:
      return "running";
    case ProcessStatus::State::Exited:
      return folly::to<std::string>("exited with status ", exitStatus());
    case ProcessStatus::State::Killed:
      return folly::to<std::string>("killed by signal ", killSignal());
    default:
      return "impossible";
  }
}

SpawnedProcess::Environment::Environment() {
  // Construct the map from the current process environment
  uint32_t nenv, i;
  const char* eq;
  const char* ent;

  for (i = 0, nenv = 0; environ[i]; i++) {
    nenv++;
  }

  map_.reserve(nenv);

  for (i = 0; environ[i]; i++) {
    ent = environ[i];
    eq = strchr(ent, '=');
    if (!eq) {
      continue;
    }

    // slice name=value into a key and a value string
    auto key = folly::StringPiece(ent, eq - ent);
    auto val = folly::StringPiece(eq + 1);

    // Replace rather than set, just in case we somehow have duplicate
    // keys in our environment array.
    map_[key.str()] = val.str();
  }
}

SpawnedProcess::Environment::Environment(
    const std::unordered_map<std::string, std::string>& map)
    : map_(map) {}

/* Constructs an envp array from a hash table.
 * The returned array occupies a single contiguous block of memory
 * such that it can be released by a single call to free(3).
 * The last element of the returned array is set to NULL for compatibility
 * with posix_spawn() */
std::unique_ptr<char*, SpawnedProcess::Deleter>
SpawnedProcess::Environment::asEnviron() const {
  size_t len = (1 + map_.size()) * sizeof(char*);

  // Make a pass through to compute the required memory size
  for (const auto& it : map_) {
    const auto& key = it.first;
    const auto& val = it.second;

    // key=value\0
    len += key.size() + 1 + val.size() + 1;
  }

  auto envp = (char**)malloc(len);
  if (!envp) {
    throw std::bad_alloc();
  }
  auto result = std::unique_ptr<char*, Deleter>(envp, Deleter());

  // Now populate
  auto buf = (char*)(envp + map_.size() + 1);
  size_t i = 0;
  for (const auto& it : map_) {
    const auto& key = it.first;
    const auto& val = it.second;

    XLOG(DBG6) << "asEnviron " << key << "=" << val;

    envp[i++] = buf;

    // key=value\0
    memcpy(buf, key.data(), key.size());
    buf += key.size();

    memcpy(buf, "=", 1);
    buf++;

    memcpy(buf, val.data(), val.size());
    buf += val.size();

    *buf = 0;
    buf++;
  }

  envp[map_.size()] = nullptr;
  return result;
}

std::string SpawnedProcess::Environment::asWin32EnvBlock() const {
  // Make a pass through to compute the required memory size
  size_t len = 1; /* for final NUL */
  for (const auto& it : map_) {
    const auto& key = it.first;
    const auto& val = it.second;

    // key=value\0
    len += key.size() + 1 + val.size() + 1;
  }

  std::string block;
  block.reserve(len);

  for (const auto& it : map_) {
    const auto& key = it.first;
    const auto& val = it.second;

    XLOG(DBG6) << "asWin32EnvBlock " << key << "=" << val;

    block.append(key);
    block.push_back('=');
    block.append(val);
    block.push_back(0);
  }

  // There's implicitly a final NUL terminator here.

  return block;
}

void SpawnedProcess::Environment::set(
    const std::string& key,
    const std::string& val) {
  map_[key] = val;
}

void SpawnedProcess::Environment::set(
    std::initializer_list<std::pair<folly::StringPiece, folly::StringPiece>>
        pairs) {
  for (auto& pair : pairs) {
    set(pair.first.str(), pair.second.str());
  }
}

void SpawnedProcess::Environment::clear() {
  map_.clear();
}

void SpawnedProcess::Environment::unset(const std::string& key) {
  map_.erase(key);
}

SpawnedProcess::Environment& SpawnedProcess::Options::environment() {
  return env_;
}

void SpawnedProcess::Options::dup2(FileDescriptor&& fd, int targetFd) {
#ifndef _WIN32
  if (targetFd == fd.fd()) {
    // Per the comments in inheritDescriptor, we cannot portably dup2
    // ourselves in the child, so we cook up an alternate source fd.
    fd = fd.duplicate();
  }
#endif
  descriptors_.emplace(std::make_pair(targetFd, std::move(fd)));
}

FileDescriptor::system_handle_type SpawnedProcess::Options::inheritDescriptor(
    FileDescriptor&& fd) {
#ifndef _WIN32
  // It is implementation dependent whether posix_spawn_file_actions_adddup2()
  // can be used to dup an fd to its own number again in the child; the
  // documentation implies that the fd is closed prior to the dup and if
  // taken literally, that implies that it will never succeed.
  // macOS and some versions of glibc do allow this to succeed, but we have
  // no way to tell if it will work.
  // What we do here instead is cook up a new number for the fd in the child,
  // taking care not to stomp on the stdio streams and trying to avoid
  // conflicting with existing descriptors.

  bool conflict = false;
  // First stage is to see whether this fd collides with any existing targets.
  // If it does, we keep duplicating the fd to get a different number until
  // we find one that doesn't conflict.
  // We keep any intermediate duplicates around in case we somehow trigger
  // the pathological case and have multiple collisions.
  // In the common case there are unlikely to be conflicts because the
  // opened fd numbers tend to be relatively high (~100 or so) and our
  // target numbers tend to be <10.
  std::vector<FileDescriptor> tempFds;

  do {
    conflict = false;
    for (auto& d : descriptors_) {
      if (d.first == fd.fd()) {
        conflict = true;
        // Try again with a different source fd number
        auto duplicated = fd.duplicate();
        tempFds.emplace_back(std::move(fd));
        fd = std::move(duplicated);
        break;
      }
    }
  } while (conflict);

  // Second stage is to determine the fd number to use in the child.
  // We avoid the stdio range, but want to prefer something small,
  // so we start with 5.
  // As above, there are unlikely to be many conflicts.
  auto target = 5;
  do {
    conflict = false;

    // Make sure it doesn't conflict with the source
    if (target == fd.fd()) {
      conflict = true;
      target++;
      continue;
    }

    // Make sure it doesn't conflict with any other descriptors
    for (auto& d : descriptors_) {
      if (d.first == target || d.second.fd() == target) {
        conflict = true;
        target++;
        break;
      }
    }
  } while (conflict);
#else
  auto target = fd.systemHandle();
#endif
  descriptors_.emplace(std::make_pair(target, std::move(fd)));
  return target;
}

void SpawnedProcess::Options::chdir(AbsolutePathPiece path) {
  cwd_ = path.copy();
}

void SpawnedProcess::Options::executablePath(AbsolutePathPiece path) {
  execPath_ = path.copy();
}

void SpawnedProcess::Options::open(
    int targetFd,
    AbsolutePathPiece path,
    OpenFileHandleOptions opts) {
  dup2(FileDescriptor::open(path, opts), targetFd);
}

void SpawnedProcess::Options::pipe(int targetFd, bool childRead) {
  if (pipes_.find(targetFd) != pipes_.end()) {
    throw std::runtime_error("targetFd is already present in pipes map");
  }

  Pipe pipe;

  if (childRead) {
    pipes_.emplace(std::make_pair(targetFd, std::move(pipe.write)));
    dup2(std::move(pipe.read), targetFd);
  } else {
    pipes_.emplace(std::make_pair(targetFd, std::move(pipe.read)));
    dup2(std::move(pipe.write), targetFd);
  }
}

void SpawnedProcess::Options::pipeStdin() {
  pipe(STDIN_FILENO, true);
}

void SpawnedProcess::Options::pipeStdout() {
  pipe(STDOUT_FILENO, false);
}

void SpawnedProcess::Options::pipeStderr() {
  pipe(STDERR_FILENO, false);
}

void SpawnedProcess::Options::nullStdin() {
  OpenFileHandleOptions opts;
  opts.readContents = 1;
  open(STDIN_FILENO, "/dev/null"_abspath, opts);
}

#ifdef _WIN32
void SpawnedProcess::Options::creationFlags(DWORD flags) {
  flags_ = flags;
}

static std::wstring build_command_line(const std::vector<std::string>& args) {
  // Here be dragons.  More gory details in http://stackoverflow.com/q/4094699
  // Surely not complete here by any means
  std::wstring result;

  for (auto& arg : args) {
    // Space separated
    if (!result.empty()) {
      result.push_back(L' ');
    }

    result.push_back(L'"');

    auto warg = multibyteToWideString(arg);
    for (auto& c : warg) {
      switch (c) {
        case L'"':
          result.append(L"\"\"\"");
          break;
        default:
          result.push_back(c);
      }
    }
    result.push_back(L'"');
  }
  return result;
}
#endif

#ifndef _WIN32
pid_t SpawnedProcess::pid() const {
  return pid_;
}

SpawnedProcess SpawnedProcess::fromExistingProcess(pid_t pid) {
  SpawnedProcess proc(pid);
  proc.waited_ = false;
  return proc;
}

SpawnedProcess::SpawnedProcess(pid_t pid) : pid_(pid) {}
#endif

SpawnedProcess::SpawnedProcess(SpawnedProcess&& other) noexcept {
  *this = std::move(other);
}

SpawnedProcess& SpawnedProcess::operator=(SpawnedProcess&& other) noexcept {
  if (&other != this) {
#ifdef _WIN32
    XCHECK_EQ(proc_, INVALID_HANDLE_VALUE);
    proc_ = other.proc_;
#else
    XCHECK_EQ(pid_, 0);
    pid_ = other.pid_;
#endif
    waited_ = other.waited_;
    status_ = other.status_;
    pipes_ = std::move(other.pipes_);
    other.waited_ = true;
  }
  return *this;
}

SpawnedProcess::SpawnedProcess(
    const std::vector<std::string>& args,
    Options&& options)
    : pipes_(std::move(options.pipes_)) {
  XCHECK(!args.empty());
#ifndef _WIN32

  posix_spawnattr_t attr;
  checkPosixError(posix_spawnattr_init(&attr), "posix_spawnattr_init");
  SCOPE_EXIT {
    posix_spawnattr_destroy(&attr);
  };

  posix_spawn_file_actions_t actions;
  checkPosixError(
      posix_spawn_file_actions_init(&actions), "posix_spawn_file_actions_init");
  SCOPE_EXIT {
    posix_spawn_file_actions_destroy(&actions);
  };

  // Reset signals to default for the child process
  posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGDEF);

  // We make a copy because posix_spawnp requires that the argv be non-const.
  // In addition, if combining chdir and executablePath we need to modify the
  // argv array.
  std::vector<std::string> argStrings = args;

  if (options.cwd_.has_value()) {
    // There isn't a portably defined way to inform posix_spawn to use an
    // alternate cwd.
    //
    // Solaris 11.3 lead the way with posix_spawn_file_actions_addchdir_np(3C).
    // glibc added support for this same function in 2.29, but that isn't yet
    // in wide circulation.  macOS doesn't have any functions for this.
    //
    // Instead, the recommendation for a multi-threaded program is to spawn a
    // helper child process that will perform the chdir and then exec the final
    // process.
    //
    // We use the shell for this.
    std::string shellCommand =
        "cd " + folly::shellQuote(options.cwd_->stringPiece()) + " && exec";

    if (options.execPath_.has_value()) {
      // When using the shell for chdir, we need to jump through a couple
      // more hoops for ARGV0 munging.
      // We're setting some environment variables to persuade zsh and bash
      // to change argv0 to our desired value.
      // Modern versions of both of those shells accept `exec -a argv0`,
      // but that behavior isn't defined by posix and since we use `/bin/sh`
      // we can't rely on anything other than the baseline bourne shell
      // behavior.
      options.environment().set("ARGV0", argStrings[0]);
      options.environment().set("BASH_ARGV0", argStrings[0]);
      // Explicitly exec the intended executable path
      argStrings[0] = options.execPath_->c_str();

      // Clear the argv0 override for posix_spawnp as we're doing it in the
      // shell and if we leave this set, we'd run execPath instead of /bin/sh
      // and that isn't at all what we want.
      options.execPath_ = std::nullopt;
    }

    for (auto& word : argStrings) {
      shellCommand.push_back(' ');
      shellCommand.append(folly::shellQuote(word));
    }

    XLOG(DBG6) << "will run : " << shellCommand;

    argStrings.clear();
    argStrings.emplace_back("/bin/sh");
    argStrings.emplace_back("-c");
    argStrings.emplace_back(std::move(shellCommand));
  }

  std::vector<char*> argv;
  argv.reserve(argStrings.size() + 1);
  for (auto& a : argStrings) {
    XLOG(DBG6) << "argv[" << argv.size() << "] = " << a;
    argv.push_back(a.data());
  }
  // The argv array is required to be NULL terminated
  argv.emplace_back(nullptr);

  // Apply our descriptor actions to the child
  for (auto& d : options.descriptors_) {
    checkPosixError(
        posix_spawn_file_actions_adddup2(&actions, d.second.fd(), d.first),
        "posix_spawn_file_actions_adddup2");
  }

  auto envp = options.env_.asEnviron();
  XLOG(DBG6) << "exec: "
             << (options.execPath_.has_value() ? options.execPath_->c_str()
                                               : argv[0]);
  auto ret = posix_spawnp(
      &pid_,
      options.execPath_.has_value() ? options.execPath_->c_str() : argv[0],
      &actions,
      &attr,
      argv.data(),
      envp.get());

  if (ret) {
    throw std::system_error(
        ret,
        std::generic_category(),
        folly::to<std::string>(
            "posix_spawnp ",
            options.execPath_.has_value() ? options.execPath_->c_str()
                                          : argv[0]));
  }
#else
  // Only handles listed in this vector will be inherited
  std::vector<HANDLE> handles;

  STARTUPINFOEXW startupInfo{};
  startupInfo.StartupInfo.cb = sizeof(STARTUPINFOEXW);
  startupInfo.StartupInfo.dwFlags = STARTF_USESTDHANDLES;

  for (auto& d : options.descriptors_) {
    auto handle = (HANDLE)d.second.handle();
    if (!SetHandleInformation(
            handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)) {
      throw makeWin32ErrorExplicit(
          GetLastError(), "SetHandleInformation failed");
    }

    // Populate stdio streams if appropriate
    switch (d.first) {
      case STDIN_FILENO:
        startupInfo.StartupInfo.hStdInput = handle;
        break;
      case STDOUT_FILENO:
        startupInfo.StartupInfo.hStdOutput = handle;
        break;
      case STDERR_FILENO:
        startupInfo.StartupInfo.hStdError = handle;
        break;
      default:;
    }

    handles.push_back(handle);
  }

  if (!startupInfo.StartupInfo.hStdInput) {
    startupInfo.StartupInfo.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    handles.push_back(startupInfo.StartupInfo.hStdInput);
  }
  if (!startupInfo.StartupInfo.hStdOutput) {
    startupInfo.StartupInfo.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    handles.push_back(startupInfo.StartupInfo.hStdOutput);
  }
  if (!startupInfo.StartupInfo.hStdError) {
    startupInfo.StartupInfo.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    handles.push_back(startupInfo.StartupInfo.hStdError);
  }

  SIZE_T size;
  InitializeProcThreadAttributeList(nullptr, 1, 0, &size);

  startupInfo.lpAttributeList = (LPPROC_THREAD_ATTRIBUTE_LIST)malloc(size);
  if (startupInfo.lpAttributeList == nullptr) {
    throw std::bad_alloc();
  }

  SCOPE_EXIT {
    free(startupInfo.lpAttributeList);
  };

  if (!InitializeProcThreadAttributeList(
          startupInfo.lpAttributeList, 1, 0, &size)) {
    throw makeWin32ErrorExplicit(
        GetLastError(), "InitializeProcThreadAttributeList failed");
  }

  SCOPE_EXIT {
    DeleteProcThreadAttributeList(startupInfo.lpAttributeList);
  };

  // Tell CreateProcess to only allow inheriting from our handle vector;
  // no other handles are inherited.
  if (!UpdateProcThreadAttribute(
          startupInfo.lpAttributeList,
          0,
          PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
          handles.data(),
          handles.size() * sizeof(HANDLE),
          nullptr,
          nullptr)) {
    throw makeWin32ErrorExplicit(
        GetLastError(), "UpdateProcThreadAttribute failed");
  }

  auto cmdLine = build_command_line(args);
  XLOGF(
      DBG6,
      "Creating the process: {}",
      wideToMultibyteString<std::string>(cmdLine));
  auto env = options.environment().asWin32EnvBlock();

  std::wstring execPath, cwd;
  if (options.execPath_) {
    execPath = multibyteToWideString(options.execPath_->stringPiece());
  }
  if (options.cwd_) {
    cwd = multibyteToWideString(options.cwd_->stringPiece());
  }
  PROCESS_INFORMATION procInfo{};
  auto status = CreateProcessW(
      options.execPath_.has_value() ? execPath.data() : NULL,
      cmdLine.data(),
      nullptr, // lpProcessAttributes
      nullptr, // lpThreadAttributes
      TRUE, // inherit the handles
      EXTENDED_STARTUPINFO_PRESENT | options.flags_.value_or(0),
      env.data(),
      options.cwd_.has_value() ? cwd.data() : NULL,
      reinterpret_cast<LPSTARTUPINFOW>(&startupInfo),
      &procInfo);

  if (!status) {
    auto errorCode = GetLastError();
    auto err = makeWin32ErrorExplicit(
        errorCode,
        fmt::format(
            "CreateProcess({}) failed",
            wideToMultibyteString<std::string>(cmdLine)));
    XLOG(ERR) << folly::exceptionStr(err);
    throw err;
  }

  CloseHandle(procInfo.hThread);
  proc_ = procInfo.hProcess;
#endif
  waited_ = false;

  // Explicitly close out the descriptors that we passed to the child
  // so that they are the only process holding open the other end of
  // the pipes that we're maintaining in pipes_.
  options.descriptors_.clear();
}

SpawnedProcess::~SpawnedProcess() {
  if (!waited_) {
    XLOG(FATAL)
        << "you must call SpawnedProcess.wait() before destroying a SpawnedProcess";
  }
}

void SpawnedProcess::detach() && {
#ifdef _WIN32
  CloseHandle(proc_);
  proc_ = INVALID_HANDLE_VALUE;
  waited_ = true;
#else
  // For posix we have no choice but to wait for the child in order to clean
  // up after it.  Ideally we'd be able to inform posix_spawn that we don't
  // want to wait for the child but there is no such option available.
  //
  // The classic way to achieve a detached/disowned child is to double fork but
  // we can't use that; we're using posix_spawn explicitly to avoid fork()
  // which is problematic especially on macOS.
  //
  // To deal with this we schedule a future_wait() so that our process can
  // periodically poll for completion.
  std::move(*this).future_wait();
#endif
}

bool SpawnedProcess::terminated() {
  if (waited_) {
    return true;
  }

#ifndef _WIN32
  int status;
  auto pid = waitpid(pid_, &status, WNOHANG);
  if (pid == pid_) {
    status_ = ProcessStatus::fromWaitStatus(status);
    waited_ = true;
  }

  if (pid == -1 && errno == ECHILD) {
    // This can happen if we are a forked child.
    // Treat this as successfully finished.
    status_ = ProcessStatus(ProcessStatus::State::Exited, 0);
    waited_ = true;
  }

#else
  auto res = WaitForSingleObject(proc_, 0);
  if (res == WAIT_OBJECT_0) {
    DWORD exitCode = 0;
    GetExitCodeProcess(proc_, &exitCode);
    status_ = ProcessStatus(ProcessStatus::State::Exited, exitCode);
    waited_ = true;
  }
#endif

  return waited_;
}

void SpawnedProcess::closeParentFd(int fdNumber) {
  pipes_.erase(fdNumber);
}

FileDescriptor SpawnedProcess::stdinFd() {
  return parentFd(STDIN_FILENO);
}

FileDescriptor SpawnedProcess::stdoutFd() {
  return parentFd(STDOUT_FILENO);
}

FileDescriptor SpawnedProcess::stderrFd() {
  return parentFd(STDERR_FILENO);
}

FileDescriptor SpawnedProcess::parentFd(int fdNumber) {
  auto it = pipes_.find(fdNumber);
  if (it != pipes_.end()) {
    FileDescriptor result = std::move(it->second);
    pipes_.erase(it);
    return result;
  }
  return FileDescriptor();
}

namespace {
/** ProcessTimeout polls the status of a SpawnedProcess
 * every poll_interval milliseconds.
 * When the process stops running it will fulfil a Promise
 * with the child status.
 */
class ProcessTimeout : public folly::AsyncTimeout {
 public:
  ProcessTimeout(
      folly::EventBase* event_base,
      SpawnedProcess proc,
      std::chrono::milliseconds poll_interval,
      std::chrono::milliseconds max_poll_interval)
      : AsyncTimeout(event_base),
        pollEveryMs_(poll_interval),
        maxPollMs_(max_poll_interval),
        subprocess_(std::move(proc)) {}

  folly::SemiFuture<ProcessStatus> initialize() {
    auto future = returnCode_.getSemiFuture();
    scheduleTimeout(pollEveryMs_.count());
    // Exponential backoff for the poll duration
    pollEveryMs_ *= 2;
    if (pollEveryMs_ > maxPollMs_) {
      pollEveryMs_ = maxPollMs_;
    }
    return future;
  }

  void timeoutExpired() noexcept override {
    if (UNLIKELY(subprocess_.terminated())) {
      returnCode_.setValue(subprocess_.wait());
      delete this;
      return;
    }
    scheduleTimeout(pollEveryMs_.count());
  }

 private:
  std::chrono::milliseconds pollEveryMs_;
  const std::chrono::milliseconds maxPollMs_;
  SpawnedProcess subprocess_;
  folly::Promise<ProcessStatus> returnCode_;
};

} // namespace

folly::SemiFuture<ProcessStatus> SpawnedProcess::future_wait(
    std::chrono::milliseconds poll_interval,
    std::chrono::milliseconds max_poll_interval) && {
  // We need to be running in a thread with an eventBase, so switch
  // over to the IOExecutor eventbase
  return folly::via(
             folly::getGlobalIOExecutor().get(),
             [process = std::move(*this),
              poll_interval,
              max_poll_interval]() mutable {
               // Create a self-owned ProcessTimeout instance and start
               // the timer.
               return (new ProcessTimeout(
                           folly::EventBaseManager::get()->getEventBase(),
                           std::move(process),
                           poll_interval,
                           max_poll_interval))
                   ->initialize();
             })
      .semi();
}

void SpawnedProcess::waitChecked() {
  auto status = wait();
  if (status.exitStatus() != 0) {
    throw std::runtime_error(
        folly::to<std::string>("Subprocess ", status.str()));
  }
}

ProcessStatus SpawnedProcess::wait() {
  if (waited_) {
    return status_;
  }

#ifndef _WIN32
  while (true) {
    int status;
    auto pid = waitpid(pid_, &status, 0);
    if (pid == pid_) {
      status_ = ProcessStatus::fromWaitStatus(status);
      waited_ = true;
      return status_;
    }

    if (errno == ECHILD) {
      // This can happen if we are a forked child.
      // Treat this as successfully finished.
      waited_ = true;
      status_ = ProcessStatus(ProcessStatus::State::Exited, 0);
      return status_;
    }

    if (errno != EINTR) {
      // We need to pretend that this child process has been waited on to
      // prevent the destructor from aborting.
      waited_ = true;
      throw std::system_error(
          errno,
          std::generic_category(),
          "SpawnedProcess::wait: waitpid returned an error");
    }
  }
#else
  auto res = WaitForSingleObject(proc_, INFINITE);
  DWORD exitCode = 0;
  switch (res) {
    case WAIT_OBJECT_0:
      GetExitCodeProcess(proc_, &exitCode);
      status_ = ProcessStatus(ProcessStatus::State::Exited, exitCode);
      waited_ = true;
      return status_;

    default:
      // Similarly to POSIX systems, we need to pretend that the child process
      // has been waited on to prevent the destructor from aborting.
      waited_ = true;
      throw makeWin32ErrorExplicit(
          GetLastError(), "WaitForSingleObject on child process handle");
  }
#endif
}

ProcessStatus SpawnedProcess::waitTimeout(std::chrono::milliseconds timeout) {
  if (waited_) {
    return status_;
  }

#ifndef _WIN32
  auto deadline = std::chrono::steady_clock::now() + timeout;
  constexpr auto maxSleep = 100ms;
  auto interval = 2ms;

  while (true) {
    int status;
    auto pid = waitpid(pid_, &status, WNOHANG);
    if (pid == pid_) {
      status_ = ProcessStatus::fromWaitStatus(status);
      waited_ = true;
      return status_;
    }

    if (pid == -1 && errno == ECHILD) {
      // This can happen if we are a forked child.
      // Treat this as successfully finished.
      status_ = ProcessStatus(ProcessStatus::State::Exited, 0);
      waited_ = true;
      return status_;
    }

    if (std::chrono::steady_clock::now() >= deadline) {
      return ProcessStatus(ProcessStatus::State::Running, 0);
    }

    std::this_thread::sleep_for(interval);
    interval = std::min(maxSleep, interval * 2);
  }
#else
  auto res = WaitForSingleObject(proc_, timeout.count());
  DWORD exitCode = 0;
  switch (res) {
    case WAIT_OBJECT_0:
      GetExitCodeProcess(proc_, &exitCode);
      status_ = ProcessStatus(ProcessStatus::State::Exited, exitCode);
      waited_ = true;
      return status_;

    case WAIT_TIMEOUT:
      return ProcessStatus(ProcessStatus::State::Running, 0);

    default:
      throw makeWin32ErrorExplicit(
          GetLastError(), "WaitForSingleObject on child process handle");
  }
#endif
}

ProcessStatus SpawnedProcess::waitOrTerminateOrKill(
    std::chrono::milliseconds waitDuration,
    std::chrono::milliseconds sigtermDuration) {
  if (waited_) {
    return status_;
  }
  waitTimeout(waitDuration);

  if (waited_) {
    return status_;
  }

  return terminateOrKill(sigtermDuration);
}

ProcessStatus SpawnedProcess::terminateOrKill(
    std::chrono::milliseconds sigtermTimeout) {
  if (waited_) {
    return status_;
  }

  terminate();
  waitTimeout(sigtermTimeout);
  if (waited_) {
    return status_;
  }

  kill();
  return wait();
}

void SpawnedProcess::kill() {
  sendSignal(
#ifdef _WIN32
      9
#else
      SIGKILL
#endif
  );
}

void SpawnedProcess::terminate() {
  sendSignal(
#ifdef _WIN32
      15
#else
      SIGTERM
#endif
  );
}

void SpawnedProcess::sendSignal(int signo) {
  if (!waited_) {
#ifndef _WIN32
    ::kill(pid_, signo);
#else
    // This should cause the target process to exit with
    // an exit status based on the signal number.
    // There is no opportunity for it to catch and shutdown
    // gracefully.
    TerminateProcess(proc_, 128 + signo);
#endif
  }
}

std::pair<std::string, std::string> SpawnedProcess::communicate(
    pipeWriteCallback writeCallback) {
#ifdef _WIN32
  return threadedCommunicate(writeCallback);
#else
  return pollingCommunicate(writeCallback);
#endif
}

#ifndef _WIN32
std::pair<std::string, std::string> SpawnedProcess::pollingCommunicate(
    pipeWriteCallback writeCallback) {
  std::unordered_map<int, std::string> outputs;

  for (auto& it : pipes_) {
    if (it.first != STDIN_FILENO) {
      // We only want output streams here
      continue;
    }
    outputs.emplace(std::make_pair(it.first, ""));
  }

  std::vector<pollfd> pfds;
  std::unordered_map<int, int> revmap;
  pfds.reserve(pipes_.size());
  revmap.reserve(pipes_.size());

  while (!pipes_.empty()) {
    revmap.clear();
    pfds.clear();

    for (auto& it : pipes_) {
      pollfd pfd;
      if (it.first == STDIN_FILENO) {
        pfd.fd = it.second.fd();
        pfd.events = POLLOUT;
      } else {
        pfd.fd = it.second.fd();
        pfd.events = POLLIN;
      }
      pfds.emplace_back(std::move(pfd));
      revmap[pfd.fd] = it.first;
    }

    int r;
    do {
      r = ::poll(pfds.data(), pfds.size(), -1);
    } while (r == -1 && errno == EINTR);
    if (r == -1) {
      throw std::system_error(errno, std::generic_category(), "poll");
    }

    for (auto& pfd : pfds) {
      if ((pfd.revents & (POLLHUP | POLLIN)) &&
          revmap[pfd.fd] != STDIN_FILENO) {
        char buf[BUFSIZ];
        auto l = ::read(pfd.fd, buf, sizeof(buf));
        if (l == -1 && (errno == EAGAIN || errno == EINTR)) {
          continue;
        }
        if (l == -1) {
          int err = errno;
          throw std::system_error(
              err, std::generic_category(), "reading from child process");
        }
        if (l == 0) {
          // Stream is done; close it out.
          pipes_.erase(revmap[pfd.fd]);
          continue;
        }
        outputs[revmap[pfd.fd]].append(buf, l);
      }

      if ((pfd.revents & POLLHUP) && revmap[pfd.fd] == STDIN_FILENO) {
        pipes_.erase(revmap[pfd.fd]);
        continue;
      }
      if ((pfd.revents & POLLOUT) && revmap[pfd.fd] == STDIN_FILENO &&
          writeCallback(pipes_.at(revmap[pfd.fd]))) {
        // We should close it
        pipes_.erase(revmap[pfd.fd]);
        continue;
      }

      if (pfd.revents & POLLERR) {
        // Something wrong with it, so close it
        pipes_.erase(revmap[pfd.fd]);
        continue;
      }
    }
  }

  auto optBuffer = [&](int fd) -> std::string {
    auto it = outputs.find(fd);
    if (it == outputs.end()) {
      return std::string();
    }
    return std::string(it->second.data(), it->second.size());
  };

  return std::make_pair(optBuffer(STDOUT_FILENO), optBuffer(STDERR_FILENO));
}
#endif

/** Spawn a thread to read from the pipe connected to the specified fd.
 * Returns a Future that will hold a string with the entire output from
 * that stream. */
folly::Future<std::string> SpawnedProcess::readPipe(int fd) {
  auto it = pipes_.find(fd);
  if (it == pipes_.end()) {
    return folly::makeFuture(std::string());
  }

  auto p = std::make_shared<folly::Promise<std::string>>();
  std::thread thr([this, fd, p]() noexcept {
    std::string result;
    p->setWith([&] {
      auto& pipe = pipes_[fd];
      while (true) {
        char buf[4096];
        auto readResult = pipe.read(buf, sizeof(buf));
        readResult.throwUnlessValue();
        auto len = readResult.value();
        if (len == 0) {
          // all done
          break;
        }
        result.append(buf, len);
      }
      return std::string(result.data(), result.size());
    });
  });

  thr.detach();
  return p->getFuture();
}

/** threadedCommunicate uses threads to read from the output streams.
 * It is intended to be used on Windows where there is no reasonable
 * way to carry out a non-blocking read on a pipe.  We compile and
 * test it on all platforms to make it easier to avoid regressions. */
std::pair<std::string, std::string> SpawnedProcess::threadedCommunicate(
    pipeWriteCallback writeCallback) {
  auto outFuture = readPipe(STDOUT_FILENO);
  auto errFuture = readPipe(STDERR_FILENO);

  auto it = pipes_.find(STDIN_FILENO);
  if (it != pipes_.end()) {
    auto& inPipe = pipes_[STDIN_FILENO];
    while (!writeCallback(inPipe)) {
      ; // keep trying to greedily write to the pipe
    }
    // Close the input stream; this typically signals the child
    // process that we're done and allows us to safely block
    // on the reads below.
    inPipe.close();
  }

  return std::make_pair(std::move(outFuture).get(), std::move(errFuture).get());
}

#ifndef _WIN32
namespace {
class Initializer {
 public:
  Initializer() {
    // Ensure that we get EPIPE rather than SIGPIPE
    ::signal(SIGPIPE, SIG_IGN);
  }
};
Initializer initializer;
} // namespace
#endif

} // namespace facebook::eden
