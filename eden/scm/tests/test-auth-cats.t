#chg-compatible
#debugruntest-incompatible

#require linux serve bucktest

  $ configure dummyssh

  $ hg init test
  $ cd test

  $ echo foo > foo
  $ hg commit -Aqm 1

  $ cert="${HGTEST_CERTDIR}/localhost.crt"
  $ cert_key="${HGTEST_CERTDIR}/localhost.key"
  $ PROXY_PORT=$(shuf -i 60002-65530 -n 1)

  $ printf "HTTP/1.1 401 Unauthorized\r\n\r\n" | ncat -lkv --ssl-cert "$cert" --ssl-key "$cert_key" localhost "$PROXY_PORT" 1>/dev/null 2>/dev/null &
  $ echo "$!" >> "$DAEMON_PIDS"

Test that cats config with type=auth reads a JSON file and sends its crypto_auth_tokens as x-auth-cats header:

  $ echo '{"crypto_auth_tokens": "my-secret-cat-token"}' > auth_cats_token
  $ auth_cats_file="$(pwd)/auth_cats_token"
  $ hg push --config http.verbose=True --config cats.myauth.path="$auth_cats_file" --config cats.myauth.type=auth --insecure --config paths.default=mononoke://localhost:$PROXY_PORT/test --config auth.mononoke.cert=$cert --config auth.mononoke.key=$cert_key --config auth.mononoke.prefix=mononoke://* 2> /dev/null | grep -o "x-auth-cats: my-secret-cat-token"
  x-auth-cats: my-secret-cat-token

Test that missing file does not cause a crash (no x-auth-cats header sent):

  $ hg push --config http.verbose=True --config cats.myauth.path="/nonexistent/file" --config cats.myauth.type=auth --insecure --config paths.default=mononoke://localhost:$PROXY_PORT/test --config auth.mononoke.cert=$cert --config auth.mononoke.key=$cert_key --config auth.mononoke.prefix=mononoke://* 2> /dev/null | grep "x-auth-cats" || echo "no x-auth-cats header"
  no x-auth-cats header

Test that no config means no x-auth-cats header:

  $ hg push --config http.verbose=True --insecure --config paths.default=mononoke://localhost:$PROXY_PORT/test --config auth.mononoke.cert=$cert --config auth.mononoke.key=$cert_key --config auth.mononoke.prefix=mononoke://* 2> /dev/null | grep "x-auth-cats" || echo "no x-auth-cats header"
  no x-auth-cats header
