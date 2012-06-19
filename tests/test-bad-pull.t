  $ "$TESTDIR/hghave" serve || exit 80

#if windows
  $ hg clone http://localhost:$HGPORT/ copy
  abort: * (glob)
  [255]
#else
  $ hg clone http://localhost:$HGPORT/ copy
  abort: error: Connection refused
  [255]
#endif

  $ test -d copy
  [1]

  $ cat > dumb.py <<EOF
  > import BaseHTTPServer, SimpleHTTPServer, os, signal
  > def run(server_class=BaseHTTPServer.HTTPServer,
  >         handler_class=SimpleHTTPServer.SimpleHTTPRequestHandler):
  >     server_address = ('localhost', int(os.environ['HGPORT']))
  >     httpd = server_class(server_address, handler_class)
  >     open("listening", "w")
  >     httpd.handle_request()
  > run()
  > EOF

  $ python dumb.py 2> log &
  $ P=$!
  $ while [ ! -f listening ]; do sleep 0; done
  $ hg clone http://localhost:$HGPORT/foo copy2
  abort: HTTP Error 404: * (glob)
  [255]
  $ wait $P
