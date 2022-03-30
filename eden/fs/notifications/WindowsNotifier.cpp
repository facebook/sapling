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

#include <folly/futures/Future.h>
#include <folly/portability/Windows.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/utils/SpawnedProcess.h"
#include "eden/fs/utils/StringConv.h"
#include "eden/fs/utils/SystemError.h"

namespace facebook::eden {
namespace {
const wchar_t EDEN_NETWORK_NOTIFICATION_TEXT[] =
    L"EdenFS is experiencing network issues";
const wchar_t EDEN_NETWORK_NOTIFICATION_TITLE[] = L"EdenFS Network Error";
const auto EDEN_NETWORK_NOTIFICATION = WindowsNotification{
    EDEN_NETWORK_NOTIFICATION_TITLE,
    EDEN_NETWORK_NOTIFICATION_TEXT};

const Guid EMenuGuid = Guid("d2ea2ee4-1a60-4fe2-a711-2c71094e3651");
constexpr UINT EMenuUid = 123;

constexpr UINT WMAPP_NOTIFYCALLBACK = WM_APP + 1;
constexpr UINT WMAPP_NOTIFYDESTROY = WM_APP + 2;

const wchar_t kWinClassNameStr[] = L"EdenFSMenu";
const wchar_t kMenuWelcomeStr[] = L"Welcome to the E-Menu";
const wchar_t kMenuCloseStr[] = L"Quit E-Menu";
const wchar_t kDebugMenu[] = L"Debug Menu";
const wchar_t kSendTestNotification[] = L"Send Test Notification";
const wchar_t kWindowTitle[] = L"EdenFSMenu";
const wchar_t kMenuToolTip[] = L"EdenFS Menu";

constexpr UINT IDM_EXIT = 124;
constexpr UINT IDM_EDENNOTIFICATION = 125;
constexpr UINT IDM_EDENDEBUGNOTIFICATION = 126;

void check(bool opResult, std::string_view context) {
  if (opResult) {
    auto errStr =
        fmt::format("{}: {}", context, win32ErrorToString(GetLastError()));
    // Exception may get swallowed by noexcept WndProc. Let's log it too.
    XLOGF(ERR, "{}: {}", context, win32ErrorToString(GetLastError()));
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

using MenuHandle =
    std::unique_ptr<std::remove_pointer_t<HMENU>, BOOL (*)(HMENU)>;

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
      kSendTestNotification);
  appendMenuEntry(
      hMenu,
      MF_BYPOSITION | MF_POPUP,
      reinterpret_cast<UINT_PTR>(subMenu.get()),
      kDebugMenu);
}

MenuHandle createEdenMenu(bool debugIsEnabled) {
  MenuHandle hMenu{
      checkNonZero(CreatePopupMenu(), "CreatePopupMenu failed"), &DestroyMenu};
  appendMenuEntry(
      hMenu.get(),
      MF_BYPOSITION | MF_STRING | MF_GRAYED,
      NULL,
      kMenuWelcomeStr);
  if (debugIsEnabled) {
    appendDebugMenu(hMenu.get());
  }
  appendMenuEntry(
      hMenu.get(), MF_BYPOSITION | MF_STRING, IDM_EXIT, kMenuCloseStr);
  return hMenu;
}

void showContextMenu(HWND hwnd, POINT pt, bool debugIsEnabled) {
  MenuHandle hMenu = createEdenMenu(debugIsEnabled);

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

void showWinNotification(
    HWND hwnd,
    const wchar_t* notifTitle,
    const wchar_t* notifBody) {
  NOTIFYICONDATAW iconData = {};
  iconData.cbSize = sizeof(iconData);
  iconData.uFlags = NIF_INFO;
  auto guid = getWindowsNotifier(hwnd)->getGuid();
  setGuidOrUid(iconData, hwnd, guid);
  // respect quiet time since this balloon did not come from a direct user
  // TODO(@cuev): maybe we should force notifications for more critical issues
  iconData.dwInfoFlags = NIIF_WARNING | NIIF_RESPECT_QUIET_TIME;
  StringCchPrintfW(
      iconData.szInfoTitle, std::size(iconData.szInfoTitle), L"%s", notifTitle);
  StringCchPrintfW(
      iconData.szInfo, std::size(iconData.szInfo), L"%s", notifBody);
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
        // add the notification icon to the system tray (event occurs when
        // Window is initially created)
        auto notifier = reinterpret_cast<CREATESTRUCT*>(lParam)->lpCreateParams;
        checkIsZero(
            SetWindowLongPtr(
                hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(notifier)),
            "SetWindowLongPtr failed");
        addNotificationIcon(hwnd);
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
            auto pendingNotif = notifier->popNextNotification();
            showWinNotification(hwnd, pendingNotif.title, pendingNotif.body);
            return 0;
          }

          case IDM_EDENDEBUGNOTIFICATION: {
            auto notifier = getWindowsNotifier(hwnd);
            const auto excp = std::exception{};
            notifier->showNetworkNotification(excp);
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
            showContextMenu(hwnd, pt, notifier->debugIsEnabled());
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
    return WindowHandle{checkNonZero(
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
    std::string_view version)
    : Notifier(std::move(edenConfig)),
      guid_{
          version == "(dev build)" ? std::nullopt
                                   : std::optional<Guid>(EMenuGuid)} {
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

WindowsNotification WindowsNotifier::popNextNotification() {
  auto ret = notifQ_.front();
  notifQ_.pop();
  return ret;
}

void WindowsNotifier::showNetworkNotification(const std::exception& /*err*/) {
  if (!updateLastShown()) {
    return;
  }
  notifQ_.push(EDEN_NETWORK_NOTIFICATION);
  PostMessage(
      hwnd_.get(),
      WM_COMMAND,
      IDM_EDENNOTIFICATION,
      reinterpret_cast<LPARAM>(this));
}

bool WindowsNotifier::debugIsEnabled() {
  return config_->getEdenConfig()->enableEdenDebugMenu.getValue();
}
} // namespace facebook::eden

#endif // _WIN32
