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
#include <windowsx.h> // @manual
#include <cstdlib>
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

// UI strings used multiple times
const wchar_t kWinClassNameStr[] = L"EdenFSMenu";
const wchar_t kToolTipDefault[] = L"EdenFS Menu";
const wchar_t kMenuAboutStr[] = L"About EdenFS";
const wchar_t kOptionEnable[] = L"Enable Notifications";

enum MenuCommand : UINT {
  IDM_ACTION_CLEAN = 124,
  IDM_ACTION_DOCTOR,
  IDM_ACTION_LIST,
  IDM_ACTION_RAGE,
  IDM_ACTION_SHOW_LOGS,
  IDM_DEBUG_GEN_NOTIFICATION,
  IDM_DEBUG_NET_NOTIFICATION,
  IDM_DEBUG_SIGNAL_END,
  IDM_DEBUG_SIGNAL_START,
  IDM_EXIT,
  IDM_INFO,
  IDM_NOTIFICATION,
  IDM_REPORT_BUG,
  IDM_SIGNAL_CHECKOUT,
  IDM_TOGGLE_NOTIFICATIONS,
};

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
      iconData.szTip, std::size(iconData.szTip), L"%s", kToolTipDefault);
  iconData.hIcon = checkNonZero(
      static_cast<HICON>(LoadImage(
          GetModuleHandle(NULL),
          MAKEINTRESOURCE(IDI_WNOTIFICATIONICON),
          IMAGE_ICON,
          32,
          32,
          LR_DEFAULTCOLOR | LR_SHARED)),
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

  // Notifications sub-menu
  MenuHandle notificationsMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      notificationsMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_DEBUG_GEN_NOTIFICATION,
      L"Generic Notification");
  appendMenuEntry(
      notificationsMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_DEBUG_NET_NOTIFICATION,
      L"Network Notification");
  appendMenuEntry(
      subMenu.get(),
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(notificationsMenu.get()),
      L"Send Test Notifications");

  // Simulation sub-menu
  MenuHandle simulationsMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      simulationsMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_DEBUG_SIGNAL_START,
      L"Starting Checkout");
  appendMenuEntry(
      simulationsMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_DEBUG_SIGNAL_END,
      L"Ending Checkout");
  appendMenuEntry(
      subMenu.get(),
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(simulationsMenu.get()),
      L"Simulate EdenFS Events");

  // Append to top-level menu
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(subMenu.get()),
      L"Debug Menu");
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

void executeShellCommand(std::string_view cmd, std::string_view params) {
  SHELLEXECUTEINFOW pExecInfo = {};
  pExecInfo.cbSize = sizeof(pExecInfo);
  // TODO(@cuev): Allow users to specify what shell they want us to
  // launch the report command with
  pExecInfo.fMask = SEE_MASK_NOASYNC;
  pExecInfo.lpVerb = L"open";
  auto cmdStr = multibyteToWideString(cmd);
  auto paramsStr = multibyteToWideString(params);
  pExecInfo.lpFile = cmdStr.c_str();
  pExecInfo.lpParameters = paramsStr.c_str();
  pExecInfo.nShow = SW_SHOWNORMAL;
  auto errStr = fmt::format("Failed to excute command: {} {}", cmd, params);
  checkNonZero(ShellExecuteExW(&pExecInfo), errStr);
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

          case IDM_NOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            showWinNotification(hwnd, *notifier->popNextNotification());
            return 0;
          }

          case IDM_DEBUG_NET_NOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            const auto excp = std::exception{};
            notifier->showNetworkNotification(excp);
            return 0;
          }

          case IDM_DEBUG_GEN_NOTIFICATION: {
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

          case IDM_INFO: {
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

          case IDM_TOGGLE_NOTIFICATIONS: {
            auto notifier = getWindowsNotifier(hwnd);
            notifier->toggleNotificationsEnabled();
            return 0;
          }

          case IDM_REPORT_BUG: {
            executeShellCommand(
                "edenfsctl", "--press-to-continue rage --report");
            return 0;
          }

          case IDM_ACTION_DOCTOR: {
            executeShellCommand("edenfsctl", "--press-to-continue doctor");
            return 0;
          }

          case IDM_ACTION_RAGE: {
            executeShellCommand("edenfsctl", "--press-to-continue rage");
            return 0;
          }

          case IDM_ACTION_LIST: {
            executeShellCommand("edenfsctl", "--press-to-continue list");
            return 0;
          }

          case IDM_ACTION_SHOW_LOGS: {
            auto homeDir = getenv("USERPROFILE");
            // Highlight the log file in explorer so that users can view the
            // logs with whatever text editor they want. I considered opening
            // the file automatically in PowerShell, but that doesn't provide a
            // great user experience.
            auto explorerArgs =
                fmt::format("/select,{}\\.eden\\logs\\edenfs.log", homeDir);
            executeShellCommand("explorer.exe", explorerArgs);
            return 0;
          }

          case IDM_ACTION_CLEAN: {
            executeShellCommand("edenfsctl", "--press-to-continue du --clean");
            return 0;
          }

          case IDM_SIGNAL_CHECKOUT: {
            auto notifier = getWindowsNotifier(hwnd);
            auto numActive = static_cast<size_t>(lParam);
            notifier->updateIconColor(numActive);
            return 0;
          }

          case IDM_DEBUG_SIGNAL_START: {
            auto notifier = getWindowsNotifier(hwnd);
            notifier->signalCheckout(1);
            return 0;
          }

          case IDM_DEBUG_SIGNAL_END: {
            auto notifier = getWindowsNotifier(hwnd);
            notifier->signalCheckout(0);
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
            POINT pt = {};
            pt.x = GET_X_LPARAM(wParam);
            pt.y = GET_Y_LPARAM(wParam);
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
            kToolTipDefault,
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

void cacheIconImages() {
  // Load all icon images so that they're cached for future use. This is
  // achieved with the LR_SHARED flag. The LR_SHARED flag makes all future
  // invocations of LoadImage load the image from cache
  LoadImage(
      GetModuleHandle(NULL),
      MAKEINTRESOURCE(IDI_WNOTIFICATIONICON),
      IMAGE_ICON,
      32,
      32,
      LR_DEFAULTCOLOR | LR_SHARED);
  LoadImage(
      GetModuleHandle(NULL),
      MAKEINTRESOURCE(IDI_ONOTIFICATIONICON),
      IMAGE_ICON,
      32,
      32,
      LR_DEFAULTCOLOR | LR_SHARED);
  LoadImage(
      GetModuleHandle(NULL),
      MAKEINTRESOURCE(IDI_RNOTIFICATIONICON),
      IMAGE_ICON,
      32,
      32,
      LR_DEFAULTCOLOR | LR_SHARED);
  LoadImage(
      GetModuleHandle(NULL),
      MAKEINTRESOURCE(IDI_GNOTIFICATIONICON),
      IMAGE_ICON,
      32,
      32,
      LR_DEFAULTCOLOR | LR_SHARED);
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
  cacheIconImages();
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

void WindowsNotifier::signalCheckout(size_t numActive) {
  PostMessage(
      hwnd_.get(),
      WM_COMMAND,
      IDM_SIGNAL_CHECKOUT,
      static_cast<LPARAM>(numActive));
}

void WindowsNotifier::updateIconColor(size_t numActive) {
  // In-progress checkouts (orange) take priority over unhealthy EdenFS mounts
  // (red). Default to white if we're healthy and have no in-progress checkouts.
  if (numActive > 0) {
    changeIconColor(IDI_ONOTIFICATIONICON);
  } else {
    changeIconColor(IDI_WNOTIFICATIONICON);
  }
}

void WindowsNotifier::changeIconColor(UINT iconType) {
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  iconData.hWnd = hwnd_.get();
  // add the icon, setting the icon, tooltip, and callback message.
  // the icon will be identified with the GUID
  iconData.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
  auto guid = getGuid();
  setGuidOrUid(iconData, hwnd_.get(), guid);
  if (iconType == IDI_ONOTIFICATIONICON) {
    StringCchPrintfW(
        iconData.szTip,
        std::size(iconData.szTip),
        L"%s",
        L"EdenFS is performing a checkout...");
  } else {
    StringCchPrintfW(
        iconData.szTip, std::size(iconData.szTip), L"%s", kToolTipDefault);
  }
  iconData.hIcon = checkNonZero(
      static_cast<HICON>(LoadImage(
          GetModuleHandle(NULL),
          MAKEINTRESOURCE(iconType),
          IMAGE_ICON,
          32,
          32,
          LR_DEFAULTCOLOR | LR_SHARED)),
      "LoadImage failed");
  // Ignore failures. It's not essential to E-Menu functioning
  (void)Shell_NotifyIconW(NIM_MODIFY, &iconData);
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
        IDM_TOGGLE_NOTIFICATIONS,
        areNotificationsEnabled() ? L"Disable Notifications" : kOptionEnable);
  } else {
    // Gray out the menu item so they can't choose to enable notifs
    appendMenuEntry(
        optionsMenu.get(),
        MF_BYPOSITION | MF_STRING | MF_GRAYED,
        NULL,
        kOptionEnable);
  }
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(optionsMenu.get()),
      L"Options");
}

void WindowsNotifier::appendActionsMenu(HMENU hMenu) {
  MenuHandle actionMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      actionMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_ACTION_DOCTOR,
      L"Diagnose EdenFS Issues (doctor)");
  appendMenuEntry(
      actionMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_ACTION_RAGE,
      L"Collect Diagnostics (rage)");
  appendMenuEntry(
      actionMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_ACTION_LIST,
      L"List Checkouts (list)");
  appendMenuEntry(
      actionMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_ACTION_CLEAN,
      L"Clean EdenFS Disk (du --clean)");
  appendMenuEntry(
      actionMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_ACTION_SHOW_LOGS,
      L"Show EdenFS Logs");

  // append actions menu to top-level menu
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(actionMenu.get()),
      L"Actions");
}

MenuHandle WindowsNotifier::createEdenMenu() {
  MenuHandle hMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      hMenu.get(),
      MF_BYPOSITION | MF_STRING | MF_GRAYED,
      NULL,
      L"Welcome to the E-Menu");
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_INFO, kMenuAboutStr);
  appendOptionsMenu(hMenu.get());
  appendActionsMenu(hMenu.get());
  if (debugIsEnabled()) {
    appendDebugMenu(hMenu.get());
  }
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_REPORT_BUG, L"Report Issue");
  appendMenuEntry(
      hMenu.get(),
      MF_BYPOSITION | MF_STRING,
      IDM_EXIT,
      L"Hide Notification Icon");
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
      IDM_NOTIFICATION,
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
  return std::wstring(L"Uptime: ") + multibyteToWideString(uptimeStr);
}

std::wstring getDaemonVersion(std::string ver) {
  return std::wstring(L"Running EdenFS ") + multibyteToWideString(ver);
}

} // namespace

std::wstring WindowsNotifier::getEdenInfoStr() {
  return getDaemonVersion(version_) + L"\n" + getDaemonUptime(startTime_);
}
} // namespace facebook::eden

#endif // _WIN32
