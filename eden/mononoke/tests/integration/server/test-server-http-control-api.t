# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

#testcases http2 http1

  $ . "${TEST_FIXTURES}/library.sh"
  $ DISABLE_HTTP_CONTROL_API=1 setup_common_config
  $ start_and_wait_for_mononoke_server
#if http2
  $ sslcurl -X POST -fsS "https://localhost:$MONONOKE_SOCKET/control/drop_bookmarks_cache"
  curl: (22) The requested URL returned error: 403* (glob)
  [22]
#else
  $ sslcurl -X POST -fsS "https://localhost:$MONONOKE_SOCKET/control/drop_bookmarks_cache" --http1.1
  curl: (22) The requested URL returned error: 403* (glob)
  [22]
#endif
