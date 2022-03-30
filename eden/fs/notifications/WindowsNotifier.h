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

class ReloadableConfig;

struct WindowDeleter {
  void operator()(HWND hwnd) {
    DestroyWindow(hwnd);
  }
};

using WindowHandle =
    std::unique_ptr<std::remove_pointer_t<HWND>, WindowDeleter>;

struct WindowsNotification {
  const wchar_t* title;
  const wchar_t* body;
};

class WindowsNotifier : public Notifier {
 public:
  explicit WindowsNotifier(
      std::shared_ptr<ReloadableConfig> edenConfig,
      std::string_view version);
  ~WindowsNotifier();

  /**
   * Show a generic network notification to the interactive user.
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
  WindowsNotification popNextNotification();

 private:
  std::optional<Guid> guid_;
  WindowHandle hwnd_;
  std::thread eventThread_;
  std::queue<WindowsNotification> notifQ_;
};

} // namespace facebook::eden
