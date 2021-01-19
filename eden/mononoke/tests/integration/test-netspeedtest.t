# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ mononoke
  $ wait_for_mononoke

Check Download
  $ sslcurl -s --header "x-netspeedtest-nbytes: 1337" -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest > output
  $ du -b output
  1337	output

Check Upload
  $ sslcurl -i --request POST https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --data-binary @output 2>/dev/null | tr -d '\r'
  HTTP/1.1 204 No Content
  Content-Length: 0
  

Check Wrong Request
  $ sslcurl -i -s --header -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest | tr -d '\r'
  HTTP/1.1 400 Bad Request
  Content-Length: 50
  
  netspeedtest: missing x-netspeedtest-nbytes header (no-eol)

Check Invalid x-netspeedtest-nbytes header value
  $ sslcurl -i -s --header "x-netspeedtest-nbytes: not-even-hex" https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest | tr -d '\r'
  HTTP/1.1 400 Bad Request
  Content-Length: 43
  
  netspeedtest: invalid digit found in string (no-eol)

Check persistent http connection with GET
  $ sslcurl -v --header "x-netspeedtest-nbytes: 1337" -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest 2>&1 | grep -o "Re-using existing connection"
  Re-using existing connection

Check persistent http connection with POST
  $ sslcurl --request POST -v -f https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest https:\/\/localhost:$MONONOKE_SOCKET/netspeedtest --data-binary @output 2>&1 | grep -o "Re-using existing connection"
  Re-using existing connection
