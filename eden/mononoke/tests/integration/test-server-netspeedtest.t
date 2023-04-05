# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

#testcases http2 http1

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ start_and_wait_for_mononoke_server
Check Download
  $ sslcurl -s --header "x-netspeedtest-nbytes: 1337" -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest > output
  $ du -b output
  1337	output

Check Upload
#if http2
  $ sslcurl -i --request POST --header "expect:" https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --data-binary @output 2>/dev/null | tr -d '\r'
  HTTP/2 200 
  x-mononoke-host: * (glob)
  date: * (glob)
  
#else
  $ sslcurl -i --request POST --header "expect:" https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --http1.1 --data-binary @output 2>/dev/null | tr -d '\r'
  HTTP/1.1 200 OK
  x-mononoke-host: * (glob)
  content-length: 0
  date: * (glob)
  
#endif

Check Wrong Request
#if http2
  $ sslcurl -i -s --header -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest | tr -d '\r'
  HTTP/2 400 
  date: * (glob)
  content-length: 86
  
  Invalid NetSpeedTest request: Request is invalid: Missing x-netspeedtest-nbytes header (no-eol)
#else
  $ sslcurl -i -s --header -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --http1.1 | tr -d '\r'
  HTTP/1.1 400 Bad Request
  content-length: 86
  date: * (glob)
  
  Invalid NetSpeedTest request: Request is invalid: Missing x-netspeedtest-nbytes header (no-eol)
#endif

#if http2
Check Invalid x-netspeedtest-nbytes header value
  $ sslcurl -i -s --header "x-netspeedtest-nbytes: not-even-hex" https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest | tr -d '\r'
  HTTP/2 400 
  date: * (glob)
  content-length: 133
  
  Invalid NetSpeedTest request: Request is invalid: Invalid x-netspeedtest-nbytes header (invalid usize): invalid digit found in string (no-eol)
#else
Check Invalid x-netspeedtest-nbytes header value
  $ sslcurl -i -s --header "x-netspeedtest-nbytes: not-even-hex" https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --http1.1 | tr -d '\r'
  HTTP/1.1 400 Bad Request
  content-length: 133
  date: * (glob)
  
  Invalid NetSpeedTest request: Request is invalid: Invalid x-netspeedtest-nbytes header (invalid usize): invalid digit found in string (no-eol)
#endif

Check persistent http connection with GET
  $ sslcurl -v --header "x-netspeedtest-nbytes: 1337" -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest 2>&1 | grep -o "Re-using existing connection"
  Re-using existing connection

Check persistent http connection with POST
  $ sslcurl --request POST -v -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --data-binary @output 2>&1 | grep -o "Re-using existing connection"
  Re-using existing connection
