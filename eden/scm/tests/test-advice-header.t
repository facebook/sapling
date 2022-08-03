#chg-compatible

#if no-windows

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ configure dummyssh
#require serve
#require bucktest

  $ hg init test
  $ cd test

  $ echo foo>foo
  $ hg addremove
  adding foo
  $ hg commit -m 1

  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cert="${HGTEST_CERTDIR}/localhost.crt"
  $ cert_key="${HGTEST_CERTDIR}/localhost.key"
  $ PROXY_PORT=$(shuf -i 60002-65530 -n 1)

  $ printf "HTTP/1.1 401 Unauthorized\r\nX-FB-Validated-X2PAuth-Advice-denied-request: advice here\r\n\r\n" | ncat -lkv --ssl-cert "$cert" --ssl-key "$cert_key" localhost "$PROXY_PORT" 1>/dev/null 2>/dev/null &
  $ echo "$!" >> "$DAEMON_PIDS"
  $ hg pull --insecure --config paths.default=mononoke://localhost:$PROXY_PORT/test --config auth.mononoke.cert=$cert --config auth.mononoke.key=$cert_key --config auth.mononoke.prefix=mononoke://*
  pulling from mononoke://localhost:*/test (glob)
  warning: connection security to localhost is disabled per current settings; communication is susceptible to eavesdropping and tampering
  abort: unexpected server response: "401 Unauthorized": advice here! (?)
  abort: unexpected server response: "401 Unauthorized":  (?)
  advice here (?)
  ! (?)
  [255]

#endif
