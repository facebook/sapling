/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#if defined(_WIN32)
#include "eden/fs/notifications/WindowsNotifier.h"
#include "eden/fs/notifications/WindowsNotifierConstants.h"

#include <commctrl.h> // @manual
#include <shellapi.h> // @manual
#include <strsafe.h> // @manual
#include <wchar.h>
#include <thread>

#include <fmt/chrono.h>
#include <fmt/xchar.h>
#include <folly/futures/Future.h>
#include <folly/portability/Windows.h>

#include "eden/common/utils/StringConv.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/SpawnedProcess.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook::eden {
namespace {

const Guid EMenuGuid = Guid("1c3dced5-4dca-4710-8b8e-851a405def31");
constexpr UINT EMenuUid = 123;

constexpr UINT WMAPP_NOTIFYCALLBACK = WM_APP + 1;
constexpr UINT WMAPP_NOTIFYDESTROY = WM_APP + 2;

const wchar_t kWinClassNameStr[] = L"EdenFSMenu";
const wchar_t kMenuWelcomeStr[] = L"Welcome to the E-Menu";
const wchar_t kMenuAboutStr[] = L"About EdenFS";
const wchar_t kMenuCloseStr[] = L"Hide Notification Icon";
const wchar_t kDebugMenu[] = L"Debug Menu";
const wchar_t kSendTestGenericNotification[] =
    L"Send Test Generic Notification";
const wchar_t kSendTestNetworkNotification[] =
    L"Send Test Network Notification";
const wchar_t kWindowTitle[] = L"EdenFSMenu";
const wchar_t kMenuToolTip[] = L"EdenFS Menu";
const wchar_t kEdenVersion[] = L"Running EdenFS ";
const wchar_t kEdenUptime[] = L"Uptime: ";
const wchar_t kMenuOptionsStr[] = L"Options";
const wchar_t kDisableNotifications[] = L"Disable Notifications";
const wchar_t kEnableNotifications[] = L"Enable Notifications";
const wchar_t kMenuReport[] = L"Report Issue";

constexpr UINT IDM_EXIT = 124;
constexpr UINT IDM_EDENNOTIFICATION = 125;
constexpr UINT IDM_EDENDEBUGNOTIFICATION = 126;
constexpr UINT IDM_EDENDEBUGNETWORKNOTIFICATION = 127;
constexpr UINT IDM_EDENINFO = 128;
constexpr UINT IDM_TOGGLENOTIFICATIONS = 129;
constexpr UINT IDM_EDENREPORT = 130;

void check(bool opResult, std::string_view context) {
  if (opResult) {
    auto errStr =
        fmt::format("{}: {}", context, win32ErrorToString(GetLastError()));
    // Exception may get swallowed by noexcept WndProc. Let's log it too.
    XLOG(ERR) << errStr;
    throw std::runtime_error(errStr);
  }
}

template <typename RET>
RET checkNonZero(RET res, std::string_view context) {
  check(res == 0, context);
  return res;
}

template <typename RET>
RET checkIsZero(RET res, std::string_view context) {
  check(res != 0, context);
  return res;
}

void setGuidOrUid(
    NOTIFYICONDATAW& iconData,
    HWND hwnd,
    const std::optional<Guid>& guid) {
  if (guid) {
    iconData.uFlags |= NIF_GUID;
    iconData.guidItem = guid.value();
  } else {
    iconData.hWnd = hwnd;
    iconData.uID = EMenuUid;
  }
}

WindowsNotifier* getWindowsNotifier(HWND hwnd) {
  return reinterpret_cast<WindowsNotifier*>(checkNonZero(
      GetWindowLongPtr(hwnd, GWLP_USERDATA), "GetWindowLongPtr failed"));
}

void registerWindowClass(
    LPCWSTR pszClassName,
    LPCWSTR pszMenuName,
    WNDPROC lpfnWndProc,
    HINSTANCE hInst) {
  WNDCLASSEXW wcex = {};
  wcex.cbSize = sizeof(wcex);
  wcex.style = 0;
  wcex.lpfnWndProc = lpfnWndProc;
  wcex.hInstance = hInst;
  wcex.hIcon = NULL;
  wcex.hCursor = NULL;
  wcex.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
  wcex.lpszMenuName = pszMenuName;
  wcex.lpszClassName = pszClassName;
  checkNonZero(
      RegisterClassExW(&wcex), "Failed to register E-Menu window class");
}

void addNotificationIcon(HWND hwnd) {
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  iconData.hWnd = hwnd;
  // add the icon, setting the icon, tooltip, and callback message.
  // the icon will be identified with the GUID
  iconData.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP;
  auto notifier = getWindowsNotifier(hwnd);
  auto guid = notifier->getGuid();
  setGuidOrUid(iconData, hwnd, guid);
  iconData.uCallbackMessage = WMAPP_NOTIFYCALLBACK;
  StringCchPrintfW(
      iconData.szTip, std::size(iconData.szTip), L"%s", kMenuToolTip);
  iconData.hIcon = checkNonZero(
      static_cast<HICON>(LoadImage(
          GetModuleHandle(NULL),
          MAKEINTRESOURCE(IDI_NOTIFICATIONICON),
          IMAGE_ICON,
          32,
          32,
          LR_DEFAULTCOLOR)),
      "LoadImage failed");

  // We might have a stale icon if eden was uncleanly terminated. We
  // should try to remove it before attempting to add a new icon.
  (void)Shell_NotifyIconW(NIM_DELETE, &iconData);
  checkNonZero(
      Shell_NotifyIconW(NIM_ADD, &iconData), "Failed to add E-Menu icon");
  iconData.uVersion = NOTIFYICON_VERSION_4;
  checkNonZero(
      Shell_NotifyIconW(NIM_SETVERSION, &iconData),
      "Failed to set E-Menu icon version");
}

void deleteNotificationIcon(HWND hwnd) {
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  auto guid = getWindowsNotifier(hwnd)->getGuid();
  setGuidOrUid(iconData, hwnd, guid);
  (void)Shell_NotifyIconW(NIM_DELETE, &iconData);
}

void restoreTooltip(HWND hwnd) {
  // After the balloon is dismissed, restore the tooltip.
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  iconData.uFlags = NIF_SHOWTIP;
  auto guid = getWindowsNotifier(hwnd)->getGuid();
  setGuidOrUid(iconData, hwnd, guid);
  checkNonZero(
      Shell_NotifyIconW(NIM_MODIFY, &iconData), "Failed to restore tooltip");
}

void appendMenuEntry(
    HMENU hMenu,
    UINT uFlags,
    UINT_PTR uIDNewItem,
    LPCWSTR lpNewItem) {
  BOOL retVal = AppendMenuW(hMenu, uFlags, uIDNewItem, lpNewItem);
  if (!retVal) {
    throw std::runtime_error(fmt::format(
        "Failed to append menu item {} with error code {}",
        wideToMultibyteString<std::string>(std::wstring_view(lpNewItem)),
        retVal));
  }
}

using MenuHandle =
    std::unique_ptr<std::remove_pointer_t<HMENU>, BOOL (*)(HMENU)>;

void appendDebugMenu(HMENU hMenu) {
  MenuHandle subMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      subMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_EDENDEBUGNOTIFICATION,
      kSendTestGenericNotification);
  appendMenuEntry(
      subMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_EDENDEBUGNETWORKNOTIFICATION,
      kSendTestNetworkNotification);
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(subMenu.get()),
      kDebugMenu);
}

void showWinNotification(HWND hwnd, const WindowsNotification& notif) {
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  iconData.uFlags = NIF_INFO;
  auto guid = getWindowsNotifier(hwnd)->getGuid();
  setGuidOrUid(iconData, hwnd, guid);
  // respect quiet time since this balloon did not come from a direct user
  // TODO(@cuev): maybe we should force notifications for more critical issues
  iconData.dwInfoFlags = NIIF_WARNING | NIIF_RESPECT_QUIET_TIME;
  std::wstring title = multibyteToWideString(notif.title);
  StringCchPrintfW(
      iconData.szInfoTitle,
      std::size(iconData.szInfoTitle),
      L"%s",
      title.c_str());
  std::wstring body = multibyteToWideString(notif.body);
  StringCchPrintfW(
      iconData.szInfo, std::size(iconData.szInfo), L"%s", body.c_str());
  checkNonZero(
      Shell_NotifyIconW(NIM_MODIFY, &iconData),
      "Failed to show E-Menu notification");
}

LRESULT CALLBACK
WndProc(HWND hwnd, UINT message, WPARAM wParam, LPARAM lParam) noexcept {
  try {
    XLOGF(DBG9, "The E-Menu received a win32 message: {}", message);
    switch (message) {
      /* Return 0 on success, throws exception on failure */
      case WM_CREATE: {
        // Set the WindowLongPtr, but don't create the E-Menu notification icon.
        // We do this elsewhere.
        auto notifier = reinterpret_cast<CREATESTRUCT*>(lParam)->lpCreateParams;
        checkIsZero(
            SetWindowLongPtr(
                hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(notifier)),
            "SetWindowLongPtr failed");
        return 0;
      }

      /* If application processes WM_DESTROY, return 0 */
      case WM_DESTROY:
        deleteNotificationIcon(hwnd);
        PostQuitMessage(0);
        return 0;

      /* If application processes WM_COMMAND, return 0 */
      case WM_COMMAND: {
        int const wmId = LOWORD(wParam);
        // Parse the menu selections:
        switch (wmId) {
          case IDM_EXIT:
            deleteNotificationIcon(hwnd);
            return 0;

          case IDM_EDENNOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            showWinNotification(hwnd, *notifier->popNextNotification());
            return 0;
          }

          case IDM_EDENDEBUGNETWORKNOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            const auto excp = std::exception{};
            notifier->showNetworkNotification(excp);
            return 0;
          }

          case IDM_EDENDEBUGNOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            constexpr std::string_view title =
                "EdenFS Test Notification - which is way too long and should be truncated!";
            constexpr std::string_view body =
                "Test notification body which is also way too long and should be truncated! "
                "But that wasn't long enough, so we'll keep typing until we reach 275 characters. "
                "Wow this is taking a while to reach this many characters. Will we realistically "
                "ever send this many characters? No.";
            constexpr std::string_view mount = "TestMountPlsIgnore";
            notifier->showNotification(title, body, mount);
            return 0;
          }

          case IDM_EDENINFO: {
            auto notifier = getWindowsNotifier(hwnd);
            auto msgBodyStr = notifier->getEdenInfoStr();
            checkNonZero(
                MessageBoxExW(
                    hwnd,
                    msgBodyStr.c_str(),
                    kMenuAboutStr,
                    MB_OK,
                    MAKELANGID(LANG_NEUTRAL, SUBLANG_NEUTRAL)),
                "Failed to populate EdenFS Info");
            return 0;
          }

          case IDM_TOGGLENOTIFICATIONS: {
            auto notifier = getWindowsNotifier(hwnd);
            notifier->toggleNotificationsEnabled();
            return 0;
          }

          case IDM_EDENREPORT: {
            SHELLEXECUTEINFOW pExecInfo = {};
            pExecInfo.cbSize = sizeof(pExecInfo);
            // TODO(@cuev): Allow users to specify what shell they want us to
            // launch the report command with
            pExecInfo.fMask = SEE_MASK_NOASYNC;
            pExecInfo.lpVerb = L"open";
            pExecInfo.lpFile = L"edenfsctl";
            pExecInfo.lpParameters = L"rage --report";
            pExecInfo.nShow = SW_SHOWNORMAL;
            checkNonZero(
                ShellExecuteExW(&pExecInfo),
                "Failed to launch EdenFS report script");
            return 0;
          }

          default:
            return DefWindowProc(hwnd, message, wParam, lParam);
        }
      }

      case WMAPP_NOTIFYCALLBACK:
        switch (LOWORD(lParam)) {
          case NIN_BALLOONTIMEOUT:
            restoreTooltip(hwnd);
            break;

          case NIN_BALLOONUSERCLICK:
            restoreTooltip(hwnd);
            break;

          case NIN_SELECT:
            // for NOTIFYICON_VERSION_4 (what we're using) clients, NIN_SELECT
            // is prerable to listening to mouse clicks and key presses
            // directly.
          case WM_CONTEXTMENU: {
            POINT const pt = {LOWORD(wParam), HIWORD(wParam)};
            auto notifier = getWindowsNotifier(hwnd);
            notifier->showContextMenu(hwnd, pt);
          } break;
        }
        return 0;

      case WMAPP_NOTIFYDESTROY:
        DestroyWindow(hwnd);
        return 0;

      default:
        return DefWindowProc(hwnd, message, wParam, lParam);
    }
  } catch (const std::exception& e) {
    XLOGF(FATAL, "Exception occurred in E-Menu WndProc: {}", e.what());
  } catch (...) {
    XLOG(FATAL, "Unknown exception occurred in E-Menu WndProc");
  }
}
int windowsEventLoop(
    HINSTANCE hInstance,
    WindowsNotifier* notifier,
    folly::Promise<WindowHandle> promise) {
  promise.setWith([&hInstance, &notifier] {
    registerWindowClass(
        kWinClassNameStr,
        MAKEINTRESOURCEW(IDC_NOTIFICATIONICON),
        WndProc,
        hInstance);
    auto windowHandle = WindowHandle{checkNonZero(
        CreateWindowW(
            kWinClassNameStr,
            kWindowTitle,
            0,
            CW_USEDEFAULT,
            0,
            0,
            0,
            NULL,
            NULL,
            hInstance,
            reinterpret_cast<LPVOID>(notifier)),
        "Failed to create E-Menu window")};
    addNotificationIcon(windowHandle.get());
    return windowHandle;
  });

  // Main message loop:
  MSG msg;
  while (GetMessage(&msg, NULL, 0, 0)) {
    TranslateMessage(&msg);
    DispatchMessage(&msg);
  }
  return 0;
}
} // namespace

WindowsNotifier::WindowsNotifier(
    std::shared_ptr<ReloadableConfig> edenConfig,
    std::string_view version,
    std::chrono::time_point<std::chrono::steady_clock> startTime)
    : Notifier(std::move(edenConfig)),
      guid_{
          version == "(dev build)" ? std::nullopt
                                   : std::optional<Guid>(EMenuGuid)},
      version_{version},
      startTime_{startTime} {
  // We only use 1 bit of the uint8_t to indicate notifs are enabled/disabled
  notificationStatus_ = notificationsEnabledInConfig()
      ? (1 << kNotificationsEnabledBit)
      : (0 << kNotificationsEnabledBit);
  // Avoids race between thread startup and hwnd_ initialization
  auto [promise, hwndFuture] = folly::makePromiseContract<WindowHandle>();
  eventThread_ = std::thread{
      windowsEventLoop, GetModuleHandle(NULL), this, std::move(promise)};
  hwnd_ = std::move(hwndFuture).get();
  XLOGF(
      DBG7,
      "EdenFS Daemon Version: {}\nGuid: {}",
      version,
      guid_ ? guid_.value().toString() : "No guid, this is a dev build");
}

WindowsNotifier::~WindowsNotifier() {
  // We cannot call DestroyWindow directly. A thread cannot use DestroyWindow to
  // destroy a window created by a different thread.
  PostMessage(
      hwnd_.release(),
      WMAPP_NOTIFYDESTROY,
      NULL,
      reinterpret_cast<LPARAM>(this));
  eventThread_.join();
}

void WindowsNotifier::appendOptionsMenu(HMENU hMenu) {
  MenuHandle optionsMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  // If notifications are disabled globally through the user's .edenrc, respect
  // that choice and don't allow them to "enable" notifs through the E-Menu
  if (notificationsEnabledInConfig()) {
    appendMenuEntry(
        optionsMenu.get(),
        MF_BYPOSITION | MF_STRING,
        IDM_TOGGLENOTIFICATIONS,
        areNotificationsEnabled() ? kDisableNotifications
                                  : kEnableNotifications);
  } else {
    // Gray out the menu item so they can't choose to enable notifs
    appendMenuEntry(
        optionsMenu.get(),
        MF_BYPOSITION | MF_STRING | MF_GRAYED,
        NULL,
        kEnableNotifications);
  }
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(optionsMenu.get()),
      kMenuOptionsStr);
}

MenuHandle WindowsNotifier::createEdenMenu() {
  MenuHandle hMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      hMenu.get(),
      MF_BYPOSITION | MF_STRING | MF_GRAYED,
      NULL,
      kMenuWelcomeStr);
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_EDENINFO, kMenuAboutStr);
  appendOptionsMenu(hMenu.get());
  if (debugIsEnabled()) {
    appendDebugMenu(hMenu.get());
  }
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_EDENREPORT, kMenuReport);
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_EXIT, kMenuCloseStr);
  return hMenu;
}

void WindowsNotifier::showContextMenu(HWND hwnd, POINT pt) {
  MenuHandle hMenu = createEdenMenu();

  /*
   * Although the Window is hidden, we still need to set it as the foreground
   * Window or the next call to TrackPopupMenuEx will fail.
   */
  checkNonZero(SetForegroundWindow(hwnd), "Failed to set foreground window");

  // respect menu drop alignment
  UINT uFlags = TPM_RIGHTBUTTON;
  if (GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0) {
    uFlags |= TPM_RIGHTALIGN;
  } else {
    uFlags |= TPM_LEFTALIGN;
  }

  checkNonZero(
      TrackPopupMenuEx(hMenu.get(), uFlags, pt.x, pt.y, hwnd, NULL),
      "TrackPopupMenuEx failed");
}

std::unique_ptr<WindowsNotification> WindowsNotifier::popNextNotification() {
  auto ret = std::move(notifQ_.front());
  notifQ_.pop();
  return ret;
}

void WindowsNotifier::showNotification(
    std::string_view notifTitle,
    std::string_view notifBody,
    std::string_view mount = {}) {
  if (!areNotificationsEnabled() || !updateLastShown()) {
    return;
  }

  std::string body{notifBody};
  std::string title{notifTitle};
  if (!mount.empty()) {
    body = fmt::format("{}: {}", mount, body);
  }

  // Win32 NOTIFYICONDATAW has a limit for the length of notification
  // titles and bodies. We need to truncate any titles/bodies that are too long
  if (body.length() > WIN32_MAX_BODY_LEN) {
    body.resize(WIN32_MAX_BODY_LEN);
  }
  if (title.length() > WIN32_MAX_TITLE_LEN) {
    title.resize(WIN32_MAX_TITLE_LEN);
  }

  auto notif = std::make_unique<WindowsNotification>();
  notif->body = std::move(body);
  notif->title = std::move(title);
  notifQ_.push(std::move(notif));
  PostMessage(
      hwnd_.get(),
      WM_COMMAND,
      IDM_EDENNOTIFICATION,
      reinterpret_cast<LPARAM>(this));
}

void WindowsNotifier::showNetworkNotification(const std::exception& /*err*/) {
  constexpr std::string_view body = "EdenFS is experiencing network issues";
  constexpr std::string_view title = "EdenFS Network Error";
  showNotification(title, body);
}

bool WindowsNotifier::debugIsEnabled() {
  return config_->getEdenConfig()->enableEdenDebugMenu.getValue();
}

bool WindowsNotifier::notificationsEnabledInConfig() {
  return config_->getEdenConfig()->enableNotifications.getValue();
}

namespace {
std::wstring getDaemonUptime(
    std::chrono::time_point<std::chrono::steady_clock> startTime) {
  auto uptimeSec = std::chrono::duration_cast<std::chrono::seconds>(
      std::chrono::steady_clock::now() - startTime);
  auto days = std::chrono::floor<std::chrono::hours>(uptimeSec) / 24;
  std::string dayStr = "";
  if (days.count() > 0) {
    dayStr = fmt::format("{} days ", days.count());
  }
  auto uptimeStr = fmt::format("{}{:%H:%M:%S}", dayStr, uptimeSec);
  return std::wstring(kEdenUptime) + multibyteToWideString(uptimeStr);
}

std::wstring getDaemonVersion(std::string ver) {
  return std::wstring(kEdenVersion) + multibyteToWideString(ver);
}

} // namespace

std::wstring WindowsNotifier::getEdenInfoStr() {
  return getDaemonVersion(version_) + L"\n" + getDaemonUptime(startTime_);
}
} // namespace facebook::eden

#endif // _WIN32
