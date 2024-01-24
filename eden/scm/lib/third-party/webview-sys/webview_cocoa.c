#include "webview.h"

#include <objc/objc-runtime.h>
#include <CoreGraphics/CoreGraphics.h>
#include <limits.h>

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wincompatible-pointer-types"
#pragma clang diagnostic ignored "-Wint-conversion"

struct webview_priv {
  id pool;
  id window;
  id webview;
  id windowDelegate;
  int should_exit;
};

struct cocoa_webview {
  const char *url;
  const char *title;
  int width;
  int height;
  int resizable;
  int debug;
  int frameless;
  int visible;
  int min_width;
  int min_height;
  int hide_instead_of_close;
  webview_external_invoke_cb_t external_invoke_cb;
  struct webview_priv priv;
  void *userdata;
};

WEBVIEW_API int webview_init(webview_t w);

WEBVIEW_API void webview_free(webview_t w) {
	free(w);
}

WEBVIEW_API void* webview_get_user_data(webview_t w) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
	return wv->userdata;
}

WEBVIEW_API webview_t webview_new(
  const char* title, const char* url, 
  int width, int height, int resizable, int debug, int frameless, int visible, int min_width, int min_height, int hide_instead_of_close,
  webview_external_invoke_cb_t external_invoke_cb, void* userdata) {
	struct cocoa_webview* wv = (struct cocoa_webview*)calloc(1, sizeof(*wv));
	wv->width = width;
	wv->height = height;
	wv->title = title;
	wv->url = url;
	wv->resizable = resizable;
	wv->debug = debug;
  wv->frameless = frameless;
  wv->visible = visible;
  wv->min_width = min_width;
  wv->min_height = min_height;
  wv->hide_instead_of_close = hide_instead_of_close;
	wv->external_invoke_cb = external_invoke_cb;
	wv->userdata = userdata;
	if (webview_init(wv) != 0) {
		webview_free(wv);
		return NULL;
	}
	return wv;
}

#define NSAlertStyleWarning 0
#define NSAlertStyleCritical 2
#define NSWindowStyleMaskResizable 8
#define NSWindowStyleMaskMiniaturizable 4
#define NSWindowStyleMaskTitled 1
#define NSWindowStyleMaskClosable 2
#define NSWindowStyleMaskFullScreen (1 << 14)
#define NSViewWidthSizable 2
#define NSViewHeightSizable 16
#define NSBackingStoreBuffered 2
#define NSEventMaskAny ULONG_MAX
#define NSEventModifierFlagCommand (1 << 20)
#define NSEventModifierFlagOption (1 << 19)
#define NSAlertStyleInformational 1
#define NSAlertFirstButtonReturn 1000
#define WKNavigationActionPolicyDownload 2
#define NSModalResponseOK 1
#define WKNavigationActionPolicyDownload 2
#define WKNavigationResponsePolicyAllow 1
#define WKUserScriptInjectionTimeAtDocumentStart 0
#define NSApplicationActivationPolicyRegular 0
#define NSApplicationDefinedEvent 15
#define NSWindowStyleMaskBorderless 0

static id get_nsstring(const char *c_str) {
  return ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSString"),
                      sel_registerName("stringWithUTF8String:"), c_str);
}

static id create_menu_item(id title, const char *action, const char *key) {
  id item =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenuItem"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id, id, id))objc_msgSend)(item, sel_registerName("initWithTitle:action:keyEquivalent:"),
               title, sel_registerName(action), get_nsstring(key));
  ((id(*)(id, SEL))objc_msgSend)(item, sel_registerName("autorelease"));

  return item;
}

static void webview_window_will_close(id self, SEL cmd, id notification) {
  struct cocoa_webview *wv =
      (struct cocoa_webview *)objc_getAssociatedObject(self, "webview");
  wv->priv.should_exit = 1;
  /***
  Since by default for `webview_loop` is set to be blocking
  we need to somehow signal the application that our
  state has changed. The activity in the `invoke_handler` does
  not interact with the `webview_loop` at all. This means that
  the `exit` wouldn't be recognized by the application until
  another event occurs like mouse movement or a key press.
  To enable the invoke_handler to notify the application
  correctly we need to send a custom event to the application.
  We are going to first create an event with the type
  NSApplicationDefined, and zero for all the other properties.
  ***/
  id event = ((id(*)(id, SEL, id, id, id, double, id, id, id, id, id))objc_msgSend)((id)objc_getClass("NSEvent"),
                  sel_registerName("otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"),
                  NSApplicationDefinedEvent,
                  (id)objc_getClass("NSZeroPoint"),
                  0, 0.0, 0, NULL, 0, 0, 0);
  id app = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                        sel_registerName("sharedApplication"));
  /***
  With a custom event crated and a pointer to the sharedApplication
  we can now send the event. We need to make sure it get's queued as
  early as possible, so we will set the argument atStart to
  the NSDate distantPast constructor. This will trigger a noop
  event on the application allowing the `webview_loop` to continue
  its current iteration.
  ***/
  ((id(*)(id, SEL, id, id))objc_msgSend)(app, sel_registerName("postEvent:atStart:"), event, 
                    ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSDate"),
                      sel_registerName("distantPast")));
}

static bool webview_window_should_close(id self, SEL cmd, id sender) {
  struct cocoa_webview *wv =
      (struct cocoa_webview *)objc_getAssociatedObject(self, "webview");

  if (wv->hide_instead_of_close) {
    webview_set_visible(wv, 0);

    return false;
  } else {
    return true;
  }
}

static void webview_external_invoke(id self, SEL cmd, id contentController,
                                    id message) {
  struct cocoa_webview *wv =
      (struct cocoa_webview *)objc_getAssociatedObject(contentController, "webview");
  if (wv == NULL || wv->external_invoke_cb == NULL) {
    return;
  }

  wv->external_invoke_cb(wv, (const char *)((id(*)(id, SEL))objc_msgSend)(
                               ((id(*)(id, SEL))objc_msgSend)(message, sel_registerName("body")),
                               sel_registerName("UTF8String")));
}

static void run_open_panel(id self, SEL cmd, id webView, id parameters,
                           id frame, void (^completionHandler)(id)) {

  id openPanel = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSOpenPanel"),
                              sel_registerName("openPanel"));

  ((id(*)(id, SEL, id))objc_msgSend)(
      openPanel, sel_registerName("setAllowsMultipleSelection:"),
      ((id(*)(id, SEL))objc_msgSend)(parameters, sel_registerName("allowsMultipleSelection")));

  ((id(*)(id, SEL, id))objc_msgSend)(openPanel, sel_registerName("setCanChooseFiles:"), 1);
  ((id(*)(id, SEL, id))objc_msgSend)(
      openPanel, sel_registerName("beginWithCompletionHandler:"), ^(id result) {
        if (result == (id)NSModalResponseOK) {
          completionHandler(((id(*)(id, SEL))objc_msgSend)(openPanel, sel_registerName("URLs")));
        } else {
          completionHandler(nil);
        }
      });
}

static void run_save_panel(id self, SEL cmd, id download, id filename,
                           void (^completionHandler)(int allowOverwrite,
                                                     id destination)) {
  id savePanel = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSSavePanel"),
                              sel_registerName("savePanel"));
  ((id(*)(id, SEL, id))objc_msgSend)(savePanel, sel_registerName("setCanCreateDirectories:"), 1);
  ((id(*)(id, SEL, id))objc_msgSend)(savePanel, sel_registerName("setNameFieldStringValue:"),
               filename);
  ((id(*)(id, SEL, id))objc_msgSend)(savePanel, sel_registerName("beginWithCompletionHandler:"),
               ^(id result) {
                 if (result == (id)NSModalResponseOK) {
                   id url = ((id(*)(id, SEL))objc_msgSend)(savePanel, sel_registerName("URL"));
                   id path = ((id(*)(id, SEL))objc_msgSend)(url, sel_registerName("path"));
                   completionHandler(1, path);
                 } else {
                   completionHandler(NO, nil);
                 }
               });
}

static void run_confirmation_panel(id self, SEL cmd, id webView, id message,
                                   id frame, void (^completionHandler)(bool)) {

  id alert =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSAlert"), sel_registerName("new"));
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setIcon:"),
               ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSImage"),
                            sel_registerName("imageNamed:"),
                            get_nsstring("NSCaution")));
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setShowsHelp:"), 0);
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setInformativeText:"), message);
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("addButtonWithTitle:"),
               get_nsstring("OK"));
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("addButtonWithTitle:"),
               get_nsstring("Cancel"));
  if (((id(*)(id, SEL))objc_msgSend)(alert, sel_registerName("runModal")) ==
      (id)NSAlertFirstButtonReturn) {
    completionHandler(true);
  } else {
    completionHandler(false);
  }
  ((id(*)(id, SEL))objc_msgSend)(alert, sel_registerName("release"));
}

static void run_alert_panel(id self, SEL cmd, id webView, id message, id frame,
                            void (^completionHandler)(void)) {
  id alert =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSAlert"), sel_registerName("new"));
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setIcon:"),
               ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSImage"),
                            sel_registerName("imageNamed:"),
                            get_nsstring("NSCaution")));
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setShowsHelp:"), 0);
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("setInformativeText:"), message);
  ((id(*)(id, SEL, id))objc_msgSend)(alert, sel_registerName("addButtonWithTitle:"),
               get_nsstring("OK"));
  ((id(*)(id, SEL))objc_msgSend)(alert, sel_registerName("runModal"));
  ((id(*)(id, SEL))objc_msgSend)(alert, sel_registerName("release"));
  completionHandler();
}

static void download_failed(id self, SEL cmd, id download, id error) {
  printf("%s",
         (const char *)((id(*)(id, SEL))objc_msgSend)(
             ((id(*)(id, SEL))objc_msgSend)(error, sel_registerName("localizedDescription")),
             sel_registerName("UTF8String")));
}

static void make_nav_policy_decision(id self, SEL cmd, id webView, id response,
                                     void (^decisionHandler)(int)) {
  if (((id(*)(id, SEL))objc_msgSend)(response, sel_registerName("canShowMIMEType")) == 0) {
    decisionHandler(WKNavigationActionPolicyDownload);
  } else {
    decisionHandler(WKNavigationResponsePolicyAllow);
  }
}

WEBVIEW_API int webview_init(webview_t w) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  wv->priv.pool = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSAutoreleasePool"),
                              sel_registerName("new"));
  ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
               sel_registerName("sharedApplication"));

  static Class __WKScriptMessageHandler;
  if(__WKScriptMessageHandler == NULL) {
    __WKScriptMessageHandler = objc_allocateClassPair(
      objc_getClass("NSObject"), "__WKScriptMessageHandler", 0);
    class_addProtocol(__WKScriptMessageHandler, objc_getProtocol("WKScriptMessageHandler"));
    class_addMethod(
        __WKScriptMessageHandler,
        sel_registerName("userContentController:didReceiveScriptMessage:"),
        (IMP)webview_external_invoke, "v@:@@");
    objc_registerClassPair(__WKScriptMessageHandler);
  }

  id scriptMessageHandler =
      ((id(*)(id, SEL))objc_msgSend)((id)__WKScriptMessageHandler, sel_registerName("new"));

  /***
   _WKDownloadDelegate is an undocumented/private protocol with methods called
   from WKNavigationDelegate
   References:
   https://github.com/WebKit/webkit/blob/master/Source/WebKit/UIProcess/API/Cocoa/_WKDownload.h
   https://github.com/WebKit/webkit/blob/master/Source/WebKit/UIProcess/API/Cocoa/_WKDownloadDelegate.h
   https://github.com/WebKit/webkit/blob/master/Tools/TestWebKitAPI/Tests/WebKitCocoa/Download.mm
   ***/

  static Class __WKDownloadDelegate;
  if(__WKDownloadDelegate == NULL) {
    __WKDownloadDelegate = objc_allocateClassPair(
      objc_getClass("NSObject"), "__WKDownloadDelegate", 0);
    class_addProtocol(__WKDownloadDelegate, objc_getProtocol("WKDownloadDelegate"));

    class_addMethod(
        __WKDownloadDelegate,
        sel_registerName("_download:decideDestinationWithSuggestedFilename:"
                        "completionHandler:"),
        (IMP)run_save_panel, "v@:@@?");
    class_addMethod(__WKDownloadDelegate,
                    sel_registerName("_download:didFailWithError:"),
                    (IMP)download_failed, "v@:@@");
    objc_registerClassPair(__WKDownloadDelegate);
  }
  
  id downloadDelegate =
      ((id(*)(id, SEL))objc_msgSend)((id)__WKDownloadDelegate, sel_registerName("new"));

  static Class __WKPreferences;
  if(__WKPreferences == NULL) {
    __WKPreferences = objc_allocateClassPair(objc_getClass("WKPreferences"),
                                                 "__WKPreferences", 0);
    objc_property_attribute_t type = {"T", "c"};
    objc_property_attribute_t ownership = {"N", ""};
    objc_property_attribute_t attrs[] = {type, ownership};
    class_replaceProperty(__WKPreferences, "developerExtrasEnabled", attrs, 2);
    objc_registerClassPair(__WKPreferences);
  }
  id wkPref = ((id(*)(id, SEL))objc_msgSend)((id)__WKPreferences, sel_registerName("new"));
  ((id(*)(id, SEL, id, id))objc_msgSend)(wkPref, sel_registerName("setValue:forKey:"),
               ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSNumber"),
                            sel_registerName("numberWithBool:"), !!wv->debug),
               ((id(*)(id, SEL, const char*))objc_msgSend)((id)objc_getClass("NSString"),
                            sel_registerName("stringWithUTF8String:"),
                            "developerExtrasEnabled"));

  id userController = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("WKUserContentController"),
                                   sel_registerName("new"));
  objc_setAssociatedObject(userController, "webview", (id)(w),
                           OBJC_ASSOCIATION_ASSIGN);
  ((id(*)(id, SEL, id, id))objc_msgSend)(
      userController, sel_registerName("addScriptMessageHandler:name:"),
      scriptMessageHandler,
      ((id(*)(id, SEL, const char*))objc_msgSend)((id)objc_getClass("NSString"),
                   sel_registerName("stringWithUTF8String:"), "invoke"));

  /***
   In order to maintain compatibility with the other 'webviews' we need to
   override window.external.invoke to call
   webkit.messageHandlers.invoke.postMessage
   ***/

  id windowExternalOverrideScript = ((id(*)(id, SEL))objc_msgSend)(
      (id)objc_getClass("WKUserScript"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id, id, id))objc_msgSend)(
      windowExternalOverrideScript,
      sel_registerName("initWithSource:injectionTime:forMainFrameOnly:"),
      get_nsstring("window.external = this; invoke = function(arg){ "
                   "webkit.messageHandlers.invoke.postMessage(arg); };"),
      WKUserScriptInjectionTimeAtDocumentStart, 0);

  ((id(*)(id, SEL, id))objc_msgSend)(userController, sel_registerName("addUserScript:"),
               windowExternalOverrideScript);

  id config = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("WKWebViewConfiguration"),
                           sel_registerName("new"));
  id processPool = ((id(*)(id, SEL))objc_msgSend)(config, sel_registerName("processPool"));
  ((id(*)(id, SEL, id))objc_msgSend)(processPool, sel_registerName("_setDownloadDelegate:"),
               downloadDelegate);
  ((id(*)(id, SEL, id))objc_msgSend)(config, sel_registerName("setProcessPool:"), processPool);
  ((id(*)(id, SEL, id))objc_msgSend)(config, sel_registerName("setUserContentController:"),
               userController);
  ((id(*)(id, SEL, id))objc_msgSend)(config, sel_registerName("setPreferences:"), wkPref);

  static Class __NSWindowDelegate;
  if(__NSWindowDelegate == NULL) {
    __NSWindowDelegate = objc_allocateClassPair(objc_getClass("NSObject"),
                                                    "__NSWindowDelegate", 0);
    class_addProtocol(__NSWindowDelegate, objc_getProtocol("NSWindowDelegate"));
    class_replaceMethod(__NSWindowDelegate, sel_registerName("windowWillClose:"),
                        (IMP)webview_window_will_close, "v@:@");
    class_replaceMethod(__NSWindowDelegate, sel_registerName("windowShouldClose:"),
                        (IMP)webview_window_should_close, "B@:@");
    objc_registerClassPair(__NSWindowDelegate);
  }

  wv->priv.windowDelegate =
      ((id(*)(id, SEL))objc_msgSend)((id)__NSWindowDelegate, sel_registerName("new"));

  objc_setAssociatedObject(wv->priv.windowDelegate, "webview", (id)(w),
                           OBJC_ASSOCIATION_ASSIGN);

  id nsTitle =
      ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSString"),
                   sel_registerName("stringWithUTF8String:"), wv->title);

  CGRect r = CGRectMake(0, 0, wv->width, wv->height);
  unsigned int style;
  if (wv->frameless) {
    style = NSWindowStyleMaskBorderless | NSWindowStyleMaskMiniaturizable;
  } else {
    style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                        NSWindowStyleMaskMiniaturizable;
  }
  if (wv->resizable) {
    style = style | NSWindowStyleMaskResizable;
  }

  wv->priv.window =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSWindow"), sel_registerName("alloc"));
  ((id(*)(id, SEL, CGRect, id, id, id))objc_msgSend)(wv->priv.window,
               sel_registerName("initWithContentRect:styleMask:backing:defer:"),
               r, style, NSBackingStoreBuffered, 0);

  ((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("autorelease"));
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setTitle:"), nsTitle);
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setDelegate:"),
               wv->priv.windowDelegate);
  ((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("center"));

  static Class __WKUIDelegate;
  if(__WKUIDelegate == NULL) {
    __WKUIDelegate = objc_allocateClassPair(objc_getClass("NSObject"), "__WKUIDelegate", 0);
    class_addProtocol(__WKUIDelegate, objc_getProtocol("WKUIDelegate"));
    class_addMethod(__WKUIDelegate,
                    sel_registerName("webView:runOpenPanelWithParameters:"
                                    "initiatedByFrame:completionHandler:"),
                    (IMP)run_open_panel, "v@:@@@?");
    class_addMethod(__WKUIDelegate,
                    sel_registerName("webView:runJavaScriptAlertPanelWithMessage:"
                                    "initiatedByFrame:completionHandler:"),
                    (IMP)run_alert_panel, "v@:@@@?");
    class_addMethod(
        __WKUIDelegate,
        sel_registerName("webView:runJavaScriptConfirmPanelWithMessage:"
                        "initiatedByFrame:completionHandler:"),
        (IMP)run_confirmation_panel, "v@:@@@?");
    objc_registerClassPair(__WKUIDelegate);
  }
  id uiDel = ((id(*)(id, SEL))objc_msgSend)((id)__WKUIDelegate, sel_registerName("new"));

  static Class __WKNavigationDelegate;
  if(__WKNavigationDelegate == NULL) {
    __WKNavigationDelegate = objc_allocateClassPair(
      objc_getClass("NSObject"), "__WKNavigationDelegate", 0);
    class_addProtocol(__WKNavigationDelegate,
                      objc_getProtocol("WKNavigationDelegate"));
    class_addMethod(
        __WKNavigationDelegate,
        sel_registerName(
            "webView:decidePolicyForNavigationResponse:decisionHandler:"),
        (IMP)make_nav_policy_decision, "v@:@@?");
    objc_registerClassPair(__WKNavigationDelegate);
  }
  id navDel = ((id(*)(id, SEL))objc_msgSend)((id)__WKNavigationDelegate, sel_registerName("new"));

  wv->priv.webview =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("WKWebView"), sel_registerName("alloc"));
  ((id(*)(id, SEL, CGRect, id))objc_msgSend)(wv->priv.webview,
               sel_registerName("initWithFrame:configuration:"), r, config);
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.webview, sel_registerName("setUIDelegate:"), uiDel);
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.webview, sel_registerName("setNavigationDelegate:"),
               navDel);

  id nsURL = ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSURL"),
                          sel_registerName("URLWithString:"),
                          get_nsstring(wv->url == NULL ? "" : wv->url));

  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.webview, sel_registerName("loadRequest:"),
               ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSURLRequest"),
                            sel_registerName("requestWithURL:"), nsURL));
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.webview, sel_registerName("setAutoresizesSubviews:"), 1);
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.webview, sel_registerName("setAutoresizingMask:"),
               (NSViewWidthSizable | NSViewHeightSizable));
  ((id(*)(id, SEL, id))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("contentView")),
               sel_registerName("addSubview:"), wv->priv.webview);

  if (wv->visible) {
    ((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("orderFrontRegardless"));
  }
  
  ((id(*)(id, SEL, CGSize))objc_msgSend)(wv->priv.window, sel_registerName("setMinSize:"), CGSizeMake(wv->min_width, wv->min_height));

  ((id(*)(id, SEL, id))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                            sel_registerName("sharedApplication")),
               sel_registerName("setActivationPolicy:"),
               NSApplicationActivationPolicyRegular);

  ((id(*)(id, SEL))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                            sel_registerName("sharedApplication")),
               sel_registerName("finishLaunching"));

  ((id(*)(id, SEL, id))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                            sel_registerName("sharedApplication")),
               sel_registerName("activateIgnoringOtherApps:"), 1);

  id menubar =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenu"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id))objc_msgSend)(menubar, sel_registerName("initWithTitle:"), get_nsstring(""));
  ((id(*)(id, SEL))objc_msgSend)(menubar, sel_registerName("autorelease"));

  id appName = ((id(*)(id, SEL))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSProcessInfo"),
                                         sel_registerName("processInfo")),
                            sel_registerName("processName"));

  id appMenuItem =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenuItem"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id, id, id))objc_msgSend)(appMenuItem,
               sel_registerName("initWithTitle:action:keyEquivalent:"), appName,
               NULL, get_nsstring(""));

  id appMenu =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenu"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("initWithTitle:"), appName);
  ((id(*)(id, SEL))objc_msgSend)(appMenu, sel_registerName("autorelease"));

  ((id(*)(id, SEL, id))objc_msgSend)(appMenuItem, sel_registerName("setSubmenu:"), appMenu);
  ((id(*)(id, SEL, id))objc_msgSend)(menubar, sel_registerName("addItem:"), appMenuItem);

  id title =
      ((id(*)(id, SEL, id))objc_msgSend)(get_nsstring("Hide "),
                   sel_registerName("stringByAppendingString:"), appName);
  id item = create_menu_item(title, "hide:", "h");
  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Hide Others"),
                          "hideOtherApplications:", "h");
  ((id(*)(id, SEL, id))objc_msgSend)(item, sel_registerName("setKeyEquivalentModifierMask:"),
               (NSEventModifierFlagOption | NSEventModifierFlagCommand));
  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("addItem:"), item);

  item =
      create_menu_item(get_nsstring("Show All"), "unhideAllApplications:", "");
  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("addItem:"), item);

  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("addItem:"),
               ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenuItem"),
                            sel_registerName("separatorItem")));

  title = ((id(*)(id, SEL, id))objc_msgSend)(get_nsstring("Quit "),
                       sel_registerName("stringByAppendingString:"), appName);
  item = create_menu_item(title, wv->frameless ? "terminate:" : "close", "q");
  ((id(*)(id, SEL, id))objc_msgSend)(appMenu, sel_registerName("addItem:"), item);

  // ----------------------------------------------------------------------------
  // Add support for Edit menu, with cut/copy/paste/select all/undo/redo.
  // This also allows keyboard shortcuts for these actions to work.
  // See https://github.com/Boscop/web-view/issues/83#issuecomment-597470641
  id editMenuItem =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenuItem"), sel_registerName("alloc"));

  ((id(*)(id, SEL, id, id, id))objc_msgSend)(editMenuItem,
               sel_registerName("initWithTitle:action:keyEquivalent:"), get_nsstring("Edit"),
               NULL, get_nsstring(""));

  id editMenu =
      ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenu"), sel_registerName("alloc"));
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("initWithTitle:"), get_nsstring("Edit"));
  ((id(*)(id, SEL))objc_msgSend)(editMenu, sel_registerName("autorelease"));

  ((id(*)(id, SEL, id))objc_msgSend)(editMenuItem, sel_registerName("setSubmenu:"), editMenu);
  ((id(*)(id, SEL, id))objc_msgSend)(menubar, sel_registerName("addItem:"), editMenuItem);

   item = create_menu_item(get_nsstring("Undo"), "undo:", "z");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Redo"), "redo:", "y");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSMenuItem"), sel_registerName("separatorItem"));
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Cut"), "cut:", "x");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Copy"), "copy:", "c");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Paste"), "paste:", "v");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);

  item = create_menu_item(get_nsstring("Select All"), "selectAll:", "a");
  ((id(*)(id, SEL, id))objc_msgSend)(editMenu, sel_registerName("addItem:"), item);
  // ----------------------------------------------------------------------------

  ((id(*)(id, SEL, id))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                            sel_registerName("sharedApplication")),
               sel_registerName("setMainMenu:"), menubar);

  wv->priv.should_exit = 0;
  return 0;
}

WEBVIEW_API int webview_loop(webview_t w, int blocking) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  id until = (blocking ? ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSDate"),
                                      sel_registerName("distantFuture"))
                       : ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSDate"),
                                      sel_registerName("distantPast")));
  id app = ((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                   sel_registerName("sharedApplication"));
  id event = ((id(*)(id, SEL, id, id, id, id))objc_msgSend)(
      app,
      sel_registerName("nextEventMatchingMask:untilDate:inMode:dequeue:"),
      ULONG_MAX, until,
      ((id(*)(id, SEL, const char*))objc_msgSend)((id)objc_getClass("NSString"),
                   sel_registerName("stringWithUTF8String:"),
                   "kCFRunLoopDefaultMode"),
      true);

  if (event) {
    ((id(*)(id, SEL, id))objc_msgSend)(((id(*)(id, SEL))objc_msgSend)((id)objc_getClass("NSApplication"),
                              sel_registerName("sharedApplication")),
                 sel_registerName("sendEvent:"), event);
  }
  return wv->priv.should_exit;
}

WEBVIEW_API int webview_eval(webview_t w, const char *js) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  ((id(*)(id, SEL, id, id))objc_msgSend)(wv->priv.webview,
               sel_registerName("evaluateJavaScript:completionHandler:"),
               get_nsstring(js), NULL);

  return 0;
}

WEBVIEW_API void webview_set_title(webview_t w, const char *title) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setTitle:"),
               get_nsstring(title));
}

WEBVIEW_API void webview_set_fullscreen(webview_t w, int fullscreen) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  unsigned long windowStyleMask = (unsigned long)((id(*)(id, SEL))objc_msgSend)(
      wv->priv.window, sel_registerName("styleMask"));
  int b = (((windowStyleMask & NSWindowStyleMaskFullScreen) ==
            NSWindowStyleMaskFullScreen)
               ? 1
               : 0);
  if (b != fullscreen) {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("toggleFullScreen:"), NULL);
  }
}

WEBVIEW_API void webview_set_maximized(webview_t w, int maximize) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  bool windowZoomStatus = (bool)((id(*)(id, SEL))objc_msgSend)(
      wv->priv.window, sel_registerName("isZoomed"));
  if (windowZoomStatus != maximize) {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("zoom:"), NULL);
  }
}

WEBVIEW_API void webview_set_minimized(webview_t w, int minimize) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  bool windowMinimizeStatus = (bool)((id(*)(id, SEL))objc_msgSend)(
      wv->priv.window, sel_registerName("isMinimized"));
  if (minimize == windowMinimizeStatus) {
    return;
  }
  if (minimize) {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("miniaturize:"), NULL);
  } else {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("deminiaturize:"), NULL);
  }
  
}

WEBVIEW_API void webview_set_visible(webview_t w, int visible) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;

  if (visible) {
    ((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("orderFrontRegardless"));
  } else {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("orderOut:"), NULL);
  }
}

WEBVIEW_API void webview_set_color(webview_t w, uint8_t r, uint8_t g,
                                   uint8_t b, uint8_t a) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  id color = ((id(*)(id, SEL, float, float, float, float))objc_msgSend)((id)objc_getClass("NSColor"),
                          sel_registerName("colorWithRed:green:blue:alpha:"),
                          (float)r / 255.0, (float)g / 255.0, (float)b / 255.0,
                          (float)a / 255.0);

  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setBackgroundColor:"), color);

  if (0.5 >= ((r / 255.0 * 299.0) + (g / 255.0 * 587.0) + (b / 255.0 * 114.0)) /
                 1000.0) {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setAppearance:"),
                 ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSAppearance"),
                              sel_registerName("appearanceNamed:"),
                              get_nsstring("NSAppearanceNameVibrantDark")));
  } else {
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setAppearance:"),
                 ((id(*)(id, SEL, id))objc_msgSend)((id)objc_getClass("NSAppearance"),
                              sel_registerName("appearanceNamed:"),
                              get_nsstring("NSAppearanceNameVibrantLight")));
  }
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("setOpaque:"), 0);
  ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window,
               sel_registerName("setTitlebarAppearsTransparent:"), 1);
}

WEBVIEW_API void webview_set_zoom_level(webview_t w, const double percentage) {
    // Ignored on Cocoa
}

WEBVIEW_API void webview_set_html(webview_t w, const char *html) {
    struct cocoa_webview* wv = (struct cocoa_webview*)w;
    ((id(*)(id, SEL, id))objc_msgSend)(wv->priv.window, sel_registerName("loadHTMLString:"),
                 get_nsstring(html));
}

static void webview_dispatch_cb(void *arg) {
  struct webview_dispatch_arg *context = (struct webview_dispatch_arg *)arg;
  (context->fn)(context->w, context->arg);
  free(context);
}

WEBVIEW_API void webview_dispatch(webview_t w, webview_dispatch_fn fn,
                                  void *arg) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  struct webview_dispatch_arg *context = (struct webview_dispatch_arg *)malloc(
      sizeof(struct webview_dispatch_arg));
  context->w = w;
  context->arg = arg;
  context->fn = fn;
  dispatch_async_f(dispatch_get_main_queue(), context, webview_dispatch_cb);
}

id read_object_property(id obj, const char* property) {
  Class cls = object_getClass(obj);
  if (cls == NULL) { return NULL; }
  objc_property_t prop = class_getProperty(cls, property);
  if (prop == NULL) { return NULL; }
  const char* getter = property_copyAttributeValue(prop, "G");
  if (getter == NULL) {
    return ((id(*)(id, SEL))objc_msgSend)(obj, sel_registerName(property));
  } else {
    return ((id(*)(id, SEL))objc_msgSend)(obj, sel_registerName(getter));
  }
}

WEBVIEW_API void webview_exit(webview_t w) {
  struct cocoa_webview* wv = (struct cocoa_webview*)w;
  wv->external_invoke_cb = NULL;
  /*
    This will try to read webview->configuration->userContentController and clear
    the associated webview which is set in the init function. It is necessary
    to avoid zombie callbacks where the controller invokes external_invoke_cb
    of a dead webview and causes a segfault (external_invoke_cb of a dead webview
    can become non-null if the memory previously owned by the webview
    is re-allocated to something else).
  */
  id config = read_object_property(wv->priv.webview, "configuration");
  if (config != NULL) {
    id controller = read_object_property(config, "userContentController");
    if (controller != NULL) {
      objc_setAssociatedObject(controller, "webview", NULL, OBJC_ASSOCIATION_ASSIGN);
    }
  }
  ((id(*)(id, SEL))objc_msgSend)(wv->priv.window, sel_registerName("close"));
}

WEBVIEW_API void webview_print_log(const char *s) { printf("%s\n", s); }

#pragma clang diagnostic pop
