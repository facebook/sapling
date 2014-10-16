#!/usr/bin/env python

"""
Small and dumb HTTP server for use in tests.
"""

from optparse import OptionParser
import BaseHTTPServer, SimpleHTTPServer, os, signal, subprocess, sys


def run(server_class=BaseHTTPServer.HTTPServer,
        handler_class=SimpleHTTPServer.SimpleHTTPRequestHandler,
        server_address=('localhost', 8000)):
    httpd = server_class(server_address, handler_class)
    httpd.serve_forever()


if __name__ == '__main__':
    parser = OptionParser()
    parser.add_option('-p', '--port', dest='port', type='int', default=8000,
        help='TCP port to listen on', metavar='PORT')
    parser.add_option('-H', '--host', dest='host', default='localhost',
        help='hostname or IP to listen on', metavar='HOST')
    parser.add_option('--pid', dest='pid',
        help='file name where the PID of the server is stored')
    parser.add_option('-f', '--foreground', dest='foreground',
        action='store_true',
        help='do not start the HTTP server in the background')

    (options, args) = parser.parse_args()

    signal.signal(signal.SIGTERM, lambda x, y: sys.exit(0))

    if options.foreground and options.pid:
        parser.error("options --pid and --foreground are mutually exclusive")

    if options.foreground:
        run(server_address=(options.host, options.port))
    else:
        # This doesn't attempt to cleanly detach the process, as it's not
        # meant to be a long-lived, independent process. As a consequence,
        # it's still part of the same process group, and keeps any file
        # descriptors it might have inherited besided stdin/stdout/stderr.
        # Trying to do things cleanly is more complicated, requires
        # OS-dependent code, and is not worth the effort.
        proc = subprocess.Popen([sys.executable, __file__, '-f',
            '-H', options.host, '-p', str(options.port)],
            stdin=open(os.devnull, 'r'),
            stdout=open(os.devnull, 'w'),
            stderr=subprocess.STDOUT)
        if options.pid:
            fp = file(options.pid, 'wb')
            fp.write(str(proc.pid) + '\n')
            fp.close()
