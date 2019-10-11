/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/logging/xlog.h>
#include "eden/fs/config/FileChangeMonitor.h"
#include "eden/fs/utils/PathFuncs.h"

#ifdef _WIN32
#include "eden/fs/win/utils/Stub.h" //@manual
#endif

namespace facebook {
namespace eden {

/**
 * CachedParsedFileMonitor provides cached access to an object of type T,
 * created by parsing a data file. The object can be accessed through
 * "getFileContents()". "getFileContents()" will reload and parse the file as
 * necessary. A throttle is applied to limit change checks to at
 * most to 1 per throttleDuration.
 *
 * The parsed value T is deduced through the Parser. The Parser and T must
 * be default constructable and provide following:
 * folly::Expected<GitIgnore, int> operator() (int fd, AbsolutePath path) const;
 *
 * CachedParsedFileMonitor is not thread safe - use external locking as
 * necessary.
 */
template <typename Parser, typename T = typename Parser::value_type>
class CachedParsedFileMonitor {
 public:
  CachedParsedFileMonitor(
      AbsolutePathPiece filePath,
      std::chrono::milliseconds throttleDuration)
      : fileChangeMonitor_{filePath, throttleDuration} {}

  /**
   * Get the parsed file contents.  If the file (or its path) has changed we
   * reload/parse it. Otherwise, we return the cached version.
   * Get the file contents for the passed filePath.  We optimize by
   * reloading/parsing the file only if the file (or its path) has
   * changed.
   * @return T created by parsing the file contents (or the errno if
   * operation failed)
   */
  folly::Expected<T, int> getFileContents(AbsolutePathPiece filePath) {
    fileChangeMonitor_.setFilePath(filePath);
    return getFileContents();
  }

  /**
   * Get the parsed file contents.  If the file (or its path) has changed we
   * reload/parse it. Otherwise, we return the cached version.
   * @return T created by parsing the file contents (or the errno if operation
   * failed)
   */
  folly::Expected<T, int> getFileContents() {
#ifndef _WIN32
    fileChangeMonitor_.invokeIfUpdated(
        [this](folly::File&& f, int errorNum, AbsolutePathPiece filePath) {
          processUpdatedFile(std::move(f), errorNum, filePath);
        });
    if (lastErrno_) {
      return folly::makeUnexpected<int>((int)lastErrno_);
    }
    return parsedData_;
#else
    NOT_IMPLEMENTED();
#endif // !_WIN32
  }

  void processUpdatedFile(
      folly::File&& f,
      int errorNum,
      AbsolutePathPiece filePath) {
    updateCount_++;
    if (errorNum != 0) {
      // Log unnecessary, FileChangeMonitor log will suffice.
      setError(errorNum);
      return;
    }
    parseFile(f.fd(), filePath);
  }

  /**
   * Get the number of times the file has been updated (simple counter).
   * Primarily for testing.
   */
  size_t getUpdateCount() const {
    return updateCount_;
  }

 private:
  /**
   * Sets the FileState error to the passed value. To assure a non-zero error
   * code, we use EIO if errNum is 0.
   */
  void setError(int errorNum) {
    lastErrno_ = errorNum ? errorNum : EIO;
    parsedData_ = T();
  }

  /**
   * Sets the FileState error to unknown error.
   */
  void setUnknownError() {
    setError(EIO);
  }

  /**
   * Parse the monitored file. We update the lastErrno_, parsedData_ and
   * fileStat_.
   */
  void parseFile(int fileDescriptor, AbsolutePathPiece filePath) {
    try {
      Parser p;
      auto rslt = p(fileDescriptor, filePath);
      // We update our results for the current file.
      if (rslt.hasError()) {
        setError(rslt.error());
      } else {
        lastErrno_ = 0;
        parsedData_ = rslt.value();
      }
    } catch (const std::exception& ex) {
      XLOG(WARN) << "error parsing file " << filePath << ": "
                 << folly::exceptionStr(ex);
      setUnknownError();
    }
  }

  T parsedData_;
  int lastErrno_{0};
  FileChangeMonitor fileChangeMonitor_;
  size_t updateCount_{0};
};
} // namespace eden
} // namespace facebook
