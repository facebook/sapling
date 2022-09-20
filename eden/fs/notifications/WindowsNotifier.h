/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>

#include "eden/fs/notifications/Notifier.h"
#include "eden/fs/utils/Guid.h"

#include <queue>

namespace facebook::eden {

constexpr size_t WIN32_MAX_TITLE_LEN = 63;
constexpr size_t WIN32_MAX_BODY_LEN = 255;
constexpr size_t kNotificationsEnabledBit = 0;

class ReloadableConfig;
struct InodePopulationReport;

struct WindowDeleter {
  void operator()(HWND hwnd) {
    DestroyWindow(hwnd);
  }
};

using WindowHandle =
    std::unique_ptr<std::remove_pointer_t<HWND>, WindowDeleter>;

using MenuHandle =
    std::unique_ptr<std::remove_pointer_t<HMENU>, BOOL (*)(HMENU)>;

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

  /*
   * Return information about the current running EdenFS daemon to display to
   * the user
   */
  std::wstring getEdenInfoStr();

  /*
   * Make the E-Menu popup menu appear to the user
   */
  void showContextMenu(HWND hwnd, POINT pt);

  /*
   * Whether notifications are enabled in the user's .edenrc
   */
  bool notificationsEnabledInConfig();

  /*
   * Whether the user has notifications enabled inside of the E-Menu
   */
  bool areNotificationsEnabled() {
    return notificationStatus_ & (1 << kNotificationsEnabledBit);
  }

  /*
   * Turn off notifications from within the E-Menu. This should only be called
   * from within the event loop thread to avoid a potential race condition.
   */
  void toggleNotificationsEnabled() {
    notificationStatus_ ^= (1 << kNotificationsEnabledBit);
  }

  void signalCheckout(size_t numActive) override;

  void registerInodePopulationReportCallback(
      std::function<std::vector<InodePopulationReport>()> callback) override;

  void updateIconColor(size_t numActive);

 private:
  void appendInodePopulationReportMenu(HMENU hMenu);
  void appendOptionsMenu(HMENU hMenu);
  void appendActionsMenu(HMENU hMenu);
  MenuHandle createEdenMenu();
  void changeIconColor(UINT iconType);
  std::optional<Guid> guid_;
  WindowHandle hwnd_;
  std::string version_;
  std::chrono::time_point<std::chrono::steady_clock> startTime_;
  std::thread eventThread_;
  std::queue<std::unique_ptr<WindowsNotification>> notifQ_;
  std::function<std::vector<InodePopulationReport>()>
      inodePopulationReportsCallback_;
  // Should only be updated from event loop thread using
  // toggleNotificationsEnabled() to avoid potential race
  uint8_t notificationStatus_;
};

} // namespace facebook::eden
