  $ "$TESTDIR/hghave" serve || exit 80

  $ hg clone http://localhost:$HGPORT/ copy
  abort: error: Connection refused
  [255]

  $ test -d copy
  [1]

  $ cat > dumb.py <<EOF
  > import BaseHTTPServer, SimpleHTTPServer, os, signal
  > def run(server_class=BaseHTTPServer.HTTPServer,
  >         handler_class=SimpleHTTPServer.SimpleHTTPRequestHandler):
  >     server_address = ('localhost', int(os.environ['HGPORT']))
  >     httpd = server_class(server_address, handler_class)
  >     os.system("hg clone http://localhost:$HGPORT/foo copy2&")
  >     httpd.handle_request()
  > run()
  > EOF

  $ python dumb.py
  localhost - - [*] code 404, message File not found (glob)
  localhost - - [*] "GET /foo?cmd=capabilities HTTP/1.1" 404 - (glob)
  abort: HTTP Error 404: * (glob)
