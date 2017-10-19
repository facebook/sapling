# hgweb/wsgicgi.py - CGI->WSGI translator
#
# Copyright 2006 Eric Hopper <hopper@omnifarious.org>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# This was originally copied from the public domain code at
# http://www.python.org/dev/peps/pep-0333/#the-server-gateway-side

from __future__ import absolute_import

from .. import (
    encoding,
    util,
)

from . import (
    common,
)

def launch(application):
    util.setbinary(util.stdin)
    util.setbinary(util.stdout)

    environ = dict(encoding.environ.iteritems())
    environ.setdefault(r'PATH_INFO', '')
    if environ.get(r'SERVER_SOFTWARE', r'').startswith(r'Microsoft-IIS'):
        # IIS includes script_name in PATH_INFO
        scriptname = environ[r'SCRIPT_NAME']
        if environ[r'PATH_INFO'].startswith(scriptname):
            environ[r'PATH_INFO'] = environ[r'PATH_INFO'][len(scriptname):]

    stdin = util.stdin
    if environ.get(r'HTTP_EXPECT', r'').lower() == r'100-continue':
        stdin = common.continuereader(stdin, util.stdout.write)

    environ[r'wsgi.input'] = stdin
    environ[r'wsgi.errors'] = util.stderr
    environ[r'wsgi.version'] = (1, 0)
    environ[r'wsgi.multithread'] = False
    environ[r'wsgi.multiprocess'] = True
    environ[r'wsgi.run_once'] = True

    if environ.get(r'HTTPS', r'off').lower() in (r'on', r'1', r'yes'):
        environ[r'wsgi.url_scheme'] = r'https'
    else:
        environ[r'wsgi.url_scheme'] = r'http'

    headers_set = []
    headers_sent = []
    out = util.stdout

    def write(data):
        if not headers_set:
            raise AssertionError("write() before start_response()")

        elif not headers_sent:
            # Before the first output, send the stored headers
            status, response_headers = headers_sent[:] = headers_set
            out.write('Status: %s\r\n' % status)
            for header in response_headers:
                out.write('%s: %s\r\n' % header)
            out.write('\r\n')

        out.write(data)
        out.flush()

    def start_response(status, response_headers, exc_info=None):
        if exc_info:
            try:
                if headers_sent:
                    # Re-raise original exception if headers sent
                    raise exc_info[0](exc_info[1], exc_info[2])
            finally:
                exc_info = None     # avoid dangling circular ref
        elif headers_set:
            raise AssertionError("Headers already set!")

        headers_set[:] = [status, response_headers]
        return write

    content = application(environ, start_response)
    try:
        for chunk in content:
            write(chunk)
        if not headers_sent:
            write('')   # send headers now if body was empty
    finally:
        getattr(content, 'close', lambda: None)()
