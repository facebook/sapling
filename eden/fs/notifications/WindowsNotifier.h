/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/notifications/Notifier.h"
#include "eden/fs/utils/Guid.h"

#include <queue>

namespace facebook::eden {

constexpr size_t WIN32_MAX_TITLE_LEN = 63;
constexpr size_t WIN32_MAX_BODY_LEN = 255;

class ReloadableConfig;

struct WindowDeleter {
  void operator()(HWND hwnd) {
    DestroyWindow(hwnd);
  }
};

using WindowHandle =
    std::unique_ptr<std::remove_pointer_t<HWND>, WindowDeleter>;

struct WindowsNotification {
  std::string title;
  std::string body;
};

class WindowsNotifier : public Notifier {
 public:
  explicit WindowsNotifier(
      std::shared_ptr<ReloadableConfig> edenConfig,
      std::string_view version,
      std::chrono::time_point<std::chrono::steady_clock> startTime);
  ~WindowsNotifier();

  /**
   * Show a generic network notification to the interactive user. The title is
   * limited to WIN32_MAX_TITLE_LEN characters, and the body + mount is limited
   * to WIN32_MAX_BODY_LEN characters. Any attempt to pass longer strings will
   * result in truncation.
   */
  virtual void showNotification(
      std::string_view notifTitle,
      std::string_view notifBody,
      std::string_view mount) override;

  /**
   * Show a network error notification to the user.
   */
  virtual void showNetworkNotification(const std::exception& err) override;

  /*
   * Get the guid associated with the notification icon
   */
  const std::optional<Guid>& getGuid() const {
    return guid_;
  }

  /*
   * Whether or not the debug menu is enabled
   */
  bool debugIsEnabled();

  /*
   * Pop the next notification from the notification queue
   */
  std::unique_ptr<WindowsNotification> popNextNotification();

  std::wstring getEdenInfoStr();

 private:
  std::optional<Guid> guid_;
  WindowHandle hwnd_;
  std::string version_;
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  std::thread eventThread_;
  std::queue<std::unique_ptr<WindowsNotification>> notifQ_;
};

} // namespace facebook::eden
