  $ hg clone http://localhost:$HGPORT/ copy
  abort: error: Connection refused
  [255]

  $ test -d copy || echo copy: No such file or directory
  copy: No such file or directory

  $ cat > dumb.py <<EOF
  > import BaseHTTPServer, SimpleHTTPServer, os, signal
  > def run(server_class=BaseHTTPServer.HTTPServer,
  >         handler_class=SimpleHTTPServer.SimpleHTTPRequestHandler):
  >     server_address = ('localhost', int(os.environ['HGPORT']))
  >     httpd = server_class(server_address, handler_class)
  >     httpd.serve_forever()
  > signal.signal(signal.SIGTERM, lambda x: sys.exit(0))
  > run()
  > EOF

  $ python dumb.py 2>/dev/null &
  $ echo $! >> $DAEMON_PIDS

give the server some time to start running

  $ sleep 1

  $ hg clone http://localhost:$HGPORT/foo copy2 2>&1
  abort: HTTP Error 404: * (glob)
  [255]

  $ kill $!
