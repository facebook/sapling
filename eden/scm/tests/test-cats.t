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

  $ printf "HTTP/1.1 401 Unauthorized\r\n\r\n" | ncat -lkv --ssl-cert "$cert" --ssl-key "$cert_key" localhost "$PROXY_PORT" 1>/dev/null 2>/dev/null &
  $ echo "$!" >> "$DAEMON_PIDS"

  $ echo '{"crypto_auth_tokens": "cats"}' > cats
  $ cats_file="$(pwd)/cats"
  $ hg push --config http.verbose=True --config cats.some.priority=1 --config cats.some.path="$cats_file" --insecure --config paths.default=mononoke://localhost:$PROXY_PORT/test --config auth.mononoke.cert=$cert --config auth.mononoke.key=$cert_key --config auth.mononoke.prefix=mononoke://* 2> /dev/null | grep -o "x-forwarded-cats: cats"
  x-forwarded-cats: cats

#endif
