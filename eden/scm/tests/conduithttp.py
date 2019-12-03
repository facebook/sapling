#!/usr/bin/env python

"""
HTTP server for use in graphql tests.
"""

import BaseHTTPServer
import json
import signal
import sys
import urlparse

# no-check-code
from optparse import OptionParser
from StringIO import StringIO


try:
    from edenscm.mercurial.server import runservice
except ImportError:
    from edenscm.mercurial.cmdutil import service as runservice

known_translations = {}
next_error_message = []


class RequestHandler(BaseHTTPServer.BaseHTTPRequestHandler):
    def handle_get_mirrored_revs_request(self, param):
        from_repo = param["from_repo"]
        from_scm = param["from_scm_type"]
        to_repo = param["to_repo"]
        to_scm = param["to_scm_type"]
        revs = param["revs"]

        if next_error_message:
            self.send_response(500)
            self.end_headers()
            self.wfile.write(json.dumps({"error": next_error_message[0]}))
            del next_error_message[0]
            return

        translations = known_translations.get(
            (from_repo, from_scm, to_repo, to_scm), {}
        )
        translated_revs = []
        self.send_response(200)
        self.end_headers()

        response = {}

        for rev in revs:
            if rev in translations:
                translated_revs.append({"from_rev": rev, "to_rev": translations[rev]})
        else:
            response["data"] = {"query": {"rev_map": translated_revs}}

        self.wfile.write(json.dumps(response))

    def do_POST(self):
        content_len = int(self.headers.getheader("content-length", 0))
        data = self.rfile.read(content_len)
        params = urlparse.parse_qs(data)

        if self.path.startswith("/graphql"):
            param = json.loads(params["variables"][0])
            if "scmquery_service_get_mirrored_revs" in params["doc"][0]:
                self.handle_get_mirrored_revs_request(param["params"])
                return
        self.send_response(500)
        self.end_headers()
        self.wfile.write(json.dumps({"error": "bad request"}))

    def get_path_comps(self):
        assert self.path.startswith("/")
        return self.path[1:].split("/")

    def update(self, cmd, comps):
        (from_repo, from_scm, to_repo, to_scm, from_rev, to_rev) = comps
        key = (from_repo, from_scm, to_repo, to_scm)
        translations = known_translations.setdefault(key, {})

        if cmd == "PUT":
            translations[from_rev] = to_rev
            self.send_response(201)
            self.end_headers()
        elif cmd == "DELETE":
            translations.pop(from_rev, None)
            self.send_response(200)
            self.end_headers()

    def do_PUT(self):
        path_comps = self.get_path_comps()
        self.log_message("%s", path_comps)
        if len(path_comps) == 6:
            self.update("PUT", path_comps)
            return
        elif len(path_comps) == 2 and path_comps[0] == "fail_next":
            # This allows tests to ask us to fail the next HTTP request
            next_error_message.append(path_comps[1])
            self.send_response(200)
            self.end_headers()
            return

        self.send_response(500)
        self.end_headers()

    def do_DELETE(self):
        path_comps = self.get_path_comps()
        if len(path_comps) == 6:
            self.update("DELETE", path_comps)
            return

        self.send_response(500)
        self.end_headers()

    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(known_translations)


class simplehttpservice(object):
    def __init__(self, host, port, port_file):
        self.address = (host, port)
        self.port_file = port_file

    def init(self):
        self.httpd = BaseHTTPServer.HTTPServer(self.address, RequestHandler)
        if self.port_file:
            with open(self.port_file, "w") as f:
                f.write("%d\n" % self.httpd.server_port)

    def run(self):
        self.httpd.serve_forever()


if __name__ == "__main__":
    parser = OptionParser()
    parser.add_option(
        "-p",
        "--port",
        dest="port",
        type="int",
        default=0,
        help="TCP port to listen on",
        metavar="PORT",
    )
    parser.add_option(
        "--port-file",
        dest="port_file",
        help="file name where the server port should be written",
    )
    parser.add_option(
        "-H",
        "--host",
        dest="host",
        default="localhost",
        help="hostname or IP to listen on",
        metavar="HOST",
    )
    parser.add_option(
        "--pid", dest="pid", help="file name where the PID of the server is stored"
    )
    parser.add_option(
        "-f",
        "--foreground",
        dest="foreground",
        action="store_true",
        help="do not start the HTTP server in the background",
    )
    parser.add_option("--daemon-postexec", action="append")

    (options, args) = parser.parse_args()

    signal.signal(signal.SIGTERM, lambda x, y: sys.exit(0))

    if options.foreground and options.pid:
        parser.error("options --pid and --foreground are mutually exclusive")

    opts = {
        "pid_file": options.pid,
        "daemon": not options.foreground,
        "daemon_postexec": options.daemon_postexec,
    }
    service = simplehttpservice(options.host, options.port, options.port_file)
    runservice(
        opts,
        initfn=service.init,
        runfn=service.run,
        runargs=["hg", "debugpython", "--", __file__] + sys.argv[1:],
    )
