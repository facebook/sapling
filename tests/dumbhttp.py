#!/usr/bin/env python

"""
Small and dumb HTTP server for use in tests.
"""

from optparse import OptionParser
import BaseHTTPServer, SimpleHTTPServer, signal, sys

from mercurial import cmdutil

class simplehttpservice(object):
    def __init__(self, host, port):
        self.address = (host, port)
    def init(self):
        self.httpd = BaseHTTPServer.HTTPServer(
            self.address, SimpleHTTPServer.SimpleHTTPRequestHandler)
    def run(self):
        self.httpd.serve_forever()

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
    parser.add_option('--daemon-pipefds')

    (options, args) = parser.parse_args()

    signal.signal(signal.SIGTERM, lambda x, y: sys.exit(0))

    if options.foreground and options.pid:
        parser.error("options --pid and --foreground are mutually exclusive")

    opts = {'pid_file': options.pid,
            'daemon': not options.foreground,
            'daemon_pipefds': options.daemon_pipefds}
    service = simplehttpservice(options.host, options.port)
    cmdutil.service(opts, initfn=service.init, runfn=service.run,
                    runargs=[sys.executable, __file__] + sys.argv[1:])
