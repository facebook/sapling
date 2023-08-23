#ifndef WEBVIEW_H
#define WEBVIEW_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef WEBVIEW_STATIC
#define WEBVIEW_API static
#else
#define WEBVIEW_API extern
#endif

#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

typedef void* webview_t;
typedef void (*webview_external_invoke_cb_t)(webview_t w, const char *arg);
typedef void (*webview_dispatch_fn)(webview_t w, void *arg);

WEBVIEW_API void webview_run(webview_t w);
WEBVIEW_API int webview_loop(webview_t w, int blocking);
WEBVIEW_API int webview_eval(webview_t w, const char *js);
WEBVIEW_API void webview_set_title(webview_t w, const char *title);
WEBVIEW_API void webview_set_fullscreen(webview_t w, int fullscreen);
WEBVIEW_API void webview_set_maximized(webview_t w, int maximize);
WEBVIEW_API void webview_set_minimized(webview_t w, int minimize);
WEBVIEW_API void webview_set_visible(webview_t w, int minimize);
WEBVIEW_API void webview_set_color(webview_t w, uint8_t r, uint8_t g,
                                   uint8_t b, uint8_t a);
WEBVIEW_API void webview_set_zoom_level(webview_t w, const double percentage);
WEBVIEW_API void webview_set_html(webview_t w, const char *html);
WEBVIEW_API void webview_dispatch(webview_t w, webview_dispatch_fn fn,
                                  void *arg);
WEBVIEW_API void webview_exit(webview_t w);
WEBVIEW_API void webview_debug(const char *format, ...);
WEBVIEW_API void webview_print_log(const char *s);

WEBVIEW_API void* webview_get_user_data(webview_t w);
WEBVIEW_API void* webview_get_window_handle(webview_t w);
WEBVIEW_API webview_t webview_new(const char* title, const char* url, int width, int height, int resizable, int debug, int frameless, int visible, int min_width, int min_height, int hide_instead_of_close, webview_external_invoke_cb_t external_invoke_cb, void* userdata);
WEBVIEW_API void webview_free(webview_t w);
WEBVIEW_API void webview_destroy(webview_t w);

// TODO WEBVIEW_API void webview_navigate(webview_t w, const char* url);

struct webview_dispatch_arg {
  webview_dispatch_fn fn;
  webview_t w;
  void *arg;
};

// Convert ASCII hex digit to a nibble (four bits, 0 - 15).
//
// Use unsigned to avoid signed overflow UB.
static inline unsigned char hex2nibble(unsigned char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  } else if (c >= 'a' && c <= 'f') {
    return 10 + (c - 'a');
  } else if (c >= 'A' && c <= 'F') {
    return 10 + (c - 'A');
  }
  return 0;
}

// Convert ASCII hex string (two characters) to byte.
//
// E.g., "0B" => 0x0B, "af" => 0xAF.
static inline char hex2char(const char* p) {
  return hex2nibble(p[0]) * 16 + hex2nibble(p[1]);
}

#ifdef __cplusplus
}
#endif

#endif // WEBVIEW_H