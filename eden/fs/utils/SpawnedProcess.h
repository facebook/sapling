/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <signal.h>
#ifndef _WIN32
#include <spawn.h>
#endif
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>
#include "eden/common/utils/Handle.h"
#include "eden/fs/utils/FileDescriptor.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/Pipe.h"

namespace facebook::eden {

// Represents the status of a process; whether it is running
// or if it has terminated, what its exit code is.
class ProcessStatus {
 public:
  enum State {
    NotStarted,
    Running,
    Exited,
    Killed,
  };

  explicit ProcessStatus(State state, int status) noexcept
      : state_(state), status_(status) {}
  ProcessStatus() = default;

  // Construct from a raw waitpid() status value
  static ProcessStatus fromWaitStatus(int rawStatus);

  // Retrieve the general running/exited/kill state
  State state() const {
    return state_;
  }

  // If the process Exited, returns the value that it
  // returned from exit(3).
  // Otherwise, returns 1.
  int exitStatus() const;

  // This only makes sense on posix systems.
  // If the process was Killed then this returns the
  // signal number that killed it.
  // Otherwise returns 0.
  int killSignal() const;

  // Returns a textual description of the state, such
  // as "not started", "running", "exited with status N"
  // and "killed by signal N".
  std::string str() const;

 private:
  State state_{NotStarted};
  int status_{0};
};

class SpawnedProcess {
 public:
  struct Deleter {
    void operator()(char** vec) const {
      free((void*)vec);
    }
  };

  class Environment {
   public:
    // Constructs an environment from the current process environment
    Environment();
    Environment(const Environment&) = default;
    /* implicit */ Environment(
        const std::unordered_map<std::string, std::string>& map);

    Environment& operator=(const Environment&) = default;

    // Returns the environment as an `environ` compatible
    // NULL-terminated array of `KEY=VALUE` C-strings.
    std::unique_ptr<char*, Deleter> asEnviron() const;

    // Returns a `CreateProcess` compatible environment block.
    // This is a single contiguous string sequenced as:
    // `KEY1=VALUE1<NUL>KEY2=VALUE2<NUL><NUL>`
    std::string asWin32EnvBlock() const;

    // Set a value in the environment
    void set(const std::string& key, const std::string& value);
    void set(
        std::initializer_list<std::pair<folly::StringPiece, folly::StringPiece>>
            pairs);

    // Remove a value from the environment
    void unset(const std::string& key);

    // Empties the environment, starting with a blank slate.
    void clear();

   private:
    std::unordered_map<std::string, std::string> map_;
  };

  class Options {
   public:
    Options() = default;
    // Not copyable
    Options(const Options&) = delete;
    Options(Options&&) = default;
    Options& operator=(const Options&) = delete;
    Options& operator=(Options&&) = default;

    // Returns a mutable, assignable reference to the environment
    // that will be used to spawn the process.
    Environment& environment();

    // Arranges to duplicate an fd from the parent as targetFd in
    // the child process.
    void dup2(FileDescriptor&& fd, int targetFd);

    // Create a pipe for communicating between the
    // parent and child process and setting it as targetFd in
    // the child.
    void pipe(int targetFd, bool childRead);

    // Set up stdin with a pipe
    void pipeStdin();

    // Set up stdout with a pipe
    void pipeStdout();

    // Set up stderr with a pipe
    void pipeStderr();

    // Set up stdin with a null device
    void nullStdin();

    // open(2) a file for the child process and make it available as targetFd.
    // `targetFd` can portably be STDIN_FILENO, STDOUT_FILENO or STDERR_FILENO.
    // Other arbitrary numbers are possible on POSIX platforms, but not on
    // Windows platforms.
    // If you need to pass streams other than the stdio streams you should
    // consider pre-opening the descriptor and calling `inheritDescriptor`
    // instead and passing the returned handle number to the spawned process
    // via its command line or through an environment variable.
    void open(int targetFd, AbsolutePathPiece path, OpenFileHandleOptions opts);

    // Arrange to set the cwd for the child process.
    // If not set, the child process to inherit the cwd from this process.
    void chdir(AbsolutePathPiece path);

    // Specifies the path to the executable.
    // This is normally produced by taking args[0] and resolving
    // it from the PATH.
    // You might want to specify this here if you already know the
    // full path but want the process to show up with an shorter
    // and simpler name for its argv[0], or otherwise wish for
    // argv[0] to vary from the executable image.
    void executablePath(AbsolutePathPiece exec);

    // Explicitly inherit `fd` and preserve its identity in
    // the child process.
    // Returns a "descriptor number" identifying it in the child.
    // This MAY NOT be the same number that it had in the parent!
    // This number is suitable for generating command line parameters to
    // allow the child to use fdopen or similar to access the
    // system handle in the child.
    FileDescriptor::system_handle_type inheritDescriptor(FileDescriptor&& fd);

#ifdef _WIN32
    void creationFlags(DWORD flags);
#endif

   private:
    // The descriptors to pass to the child
    std::unordered_map<int, FileDescriptor> descriptors_;
    // The environment to pass to the child
    Environment env_;
    // The parent side of any pipes configured
    std::unordered_map<int, FileDescriptor> pipes_;
    // The current working directory to set in the child
    std::optional<AbsolutePath> cwd_;
    // Alternative executable image path
    std::optional<AbsolutePath> execPath_;
#ifdef _WIN32
    std::optional<DWORD> flags_;
#endif

    friend class SpawnedProcess;
  };

  SpawnedProcess() = default;
  ~SpawnedProcess();

  // Attempt to spawn the process defined by `args` and `options`.
  // Note that `options` is moved in because it owns any redirected
  // descriptors that were configured.
  explicit SpawnedProcess(
      const std::vector<std::string>& args,
      Options&& options = Options());

  SpawnedProcess(const SpawnedProcess&) = delete;
  SpawnedProcess& operator=(const SpawnedProcess&) = delete;

  SpawnedProcess(SpawnedProcess&& other) noexcept;
  SpawnedProcess& operator=(SpawnedProcess&& other) noexcept;

#ifndef _WIN32
  // Construct from an already-running process id
  static SpawnedProcess fromExistingProcess(pid_t pid);
  explicit SpawnedProcess(pid_t pid);
#endif

  // Check to see if the process has terminated.
  // Does not block.  Returns true if the process has
  // terminated, false otherwise.
  bool terminated();

  // Wait for the process to terminate and return its
  // exit status.  If the process has already terminated,
  // immediately returns its exit status.
  ProcessStatus wait();

  // Wait for the process to terminate.  If it didn't exit with
  // status==0 then throw an exception.
  void waitChecked();

  // Wait up to `timeout` for the process to terminate.
  ProcessStatus waitTimeout(std::chrono::milliseconds timeout);

  /**
   * Call `waitpid` non-blockingly up to `waitTimeout`. If the process hasn't
   * terminated after that, fall back on `terminateOrKill` with
   * `sigtermTimeoutSeconds`.
   */
  ProcessStatus waitOrTerminateOrKill(
      std::chrono::milliseconds waitTimeout,
      std::chrono::milliseconds sigtermTimeout);

  /**
   * Send the SIGTERM to terminate the process, poll `waitpid` non-blockingly
   * several times up to `sigtermTimeout`. If the process hasn't terminated
   * after that, send SIGKILL to kill the process and call `waitpid` blockingly.
   * Return the exit code of process.
   */
  ProcessStatus terminateOrKill(std::chrono::milliseconds sigtermTimeout);

  // Consumes the process and returns a SemiFuture that will yield its
  // resultant exit status when the process completes.
  // The SemiFuture is implemented by polling the return code at the specified
  // poll_interval (default is 10ms), with exponential backoff up to the
  // specified maximum poll interval.
  // The polling is managed by a timer registered with the global IO Executor.
  folly::SemiFuture<ProcessStatus> future_wait(
      std::chrono::milliseconds poll_interval = std::chrono::milliseconds(10),
      std::chrono::milliseconds max_poll_interval =
          std::chrono::seconds(10)) &&;

  // Disassociate from the running process.
  // We will no longer be able to wait for it to complete.
  // This is implemented in terms of future_wait() on POSIX systems.
  void detach() &&;

  // Terminates the process with SIGKILL (calls `sendSignal(SIGKILL)`)
  void kill();

  // Terminates the process with SIGTERM (calls `sendSignal(SIGTERM)`).
  void terminate();

  // POSIX: Send an arbitrary signal to the process.  Depending on the
  // signal, the process may catch/handle the signal and may not immediately
  // terminate.
  //
  // Windows: immediately terminate the process and set its exit code to
  // signo+128.
  void sendSignal(int signo);

  // The pipeWriteCallback is called by communicate when it is safe to write
  // data to the pipe.  The callback should then attempt to write to it.
  // The callback must return true when it has nothing more
  // to write to the input of the child.  This will cause the
  // pipe to be closed.
  // Note that the pipe may be non-blocking, and you must not loop attempting
  // to write data to the pipe - the caller will arrange to call you again
  // if you return false (e.g. after a partial write).
  using pipeWriteCallback = std::function<bool(FileDescriptor&)>;

  /** SpawnedProcess::communicate() performs a read/write operation.
   * The provided pipeWriteCallback allows sending data to the input stream.
   * communicate() will return with the pair of output and error streams once
   * they have been completely consumed. */
  std::pair<std::string, std::string> communicate(
      pipeWriteCallback writeCallback = [](FileDescriptor&) {
        // If not provided by the caller, we're just going to close the input
        // stream
        return true;
      });

  // these are public for the sake of testing.  You should use the
  // communicate() method instead of calling these directly.
  std::pair<std::string, std::string> pollingCommunicate(
      pipeWriteCallback writable);
  std::pair<std::string, std::string> threadedCommunicate(
      pipeWriteCallback writable);

  // fdNumber is the descriptor as seen by the child; this method
  // closes the parent side of that numbered descriptor.
  void closeParentFd(int fdNumber);

  // Take ownership of the descriptor representing the stdin stream
  FileDescriptor stdinFd();

  // Take ownership of the description representing the stdout stream
  FileDescriptor stdoutFd();

  // Take ownership of the description representing the stderr stream
  FileDescriptor stderrFd();

  // fdNumber is the descriptor as seen by the child; this method
  // return the parent side of that numbered descriptor.
  FileDescriptor parentFd(int fdNumber);

#ifndef _WIN32
  // Retrieve the process id of the child
  pid_t pid() const;
#endif

 private:
#ifndef _WIN32
  pid_t pid_{0};
#else
  ProcessHandle proc_{};
#endif
  bool waited_{true};
  ProcessStatus status_;
  std::unordered_map<int, FileDescriptor> pipes_;

  folly::Future<std::string> readPipe(int fd);
};

} // namespace facebook::eden
