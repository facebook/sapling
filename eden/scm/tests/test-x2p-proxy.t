 #chg-compatible 

#if no-windows

  $ hg init test
  $ cd test
  $ printf "HTTP/1.1 200 OK\r\n\r\n" | socat -v -d -d UNIX-LISTEN:socat.sock - 2> output >/dev/null &
  $ hg --config auth_proxy.unix_socket_path=socat.sock --config auth_proxy.unix_socket_domains=localhost --config edenapi.url=http://localhost/edenapi/ debughttp >/dev/null 2>&1

  $ grep -oi "+x2pagentd" output
  +x2pagentd

#endif
