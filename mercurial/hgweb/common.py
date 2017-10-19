# hgweb/common.py - Utility functions needed by hgweb_mod and hgwebdir_mod
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import base64
import errno
import mimetypes
import os

from .. import (
    encoding,
    pycompat,
    util,
)

httpserver = util.httpserver

HTTP_OK = 200
HTTP_NOT_MODIFIED = 304
HTTP_BAD_REQUEST = 400
HTTP_UNAUTHORIZED = 401
HTTP_FORBIDDEN = 403
HTTP_NOT_FOUND = 404
HTTP_METHOD_NOT_ALLOWED = 405
HTTP_SERVER_ERROR = 500


def ismember(ui, username, userlist):
    """Check if username is a member of userlist.

    If userlist has a single '*' member, all users are considered members.
    Can be overridden by extensions to provide more complex authorization
    schemes.
    """
    return userlist == ['*'] or username in userlist

def checkauthz(hgweb, req, op):
    '''Check permission for operation based on request data (including
    authentication info). Return if op allowed, else raise an ErrorResponse
    exception.'''

    user = req.env.get('REMOTE_USER')

    deny_read = hgweb.configlist('web', 'deny_read')
    if deny_read and (not user or ismember(hgweb.repo.ui, user, deny_read)):
        raise ErrorResponse(HTTP_UNAUTHORIZED, 'read not authorized')

    allow_read = hgweb.configlist('web', 'allow_read')
    if allow_read and (not ismember(hgweb.repo.ui, user, allow_read)):
        raise ErrorResponse(HTTP_UNAUTHORIZED, 'read not authorized')

    if op == 'pull' and not hgweb.allowpull:
        raise ErrorResponse(HTTP_UNAUTHORIZED, 'pull not authorized')
    elif op == 'pull' or op is None: # op is None for interface requests
        return

    # enforce that you can only push using POST requests
    if req.env['REQUEST_METHOD'] != 'POST':
        msg = 'push requires POST request'
        raise ErrorResponse(HTTP_METHOD_NOT_ALLOWED, msg)

    # require ssl by default for pushing, auth info cannot be sniffed
    # and replayed
    scheme = req.env.get('wsgi.url_scheme')
    if hgweb.configbool('web', 'push_ssl') and scheme != 'https':
        raise ErrorResponse(HTTP_FORBIDDEN, 'ssl required')

    deny = hgweb.configlist('web', 'deny_push')
    if deny and (not user or ismember(hgweb.repo.ui, user, deny)):
        raise ErrorResponse(HTTP_UNAUTHORIZED, 'push not authorized')

    allow = hgweb.configlist('web', 'allow-push')
    if not (allow and ismember(hgweb.repo.ui, user, allow)):
        raise ErrorResponse(HTTP_UNAUTHORIZED, 'push not authorized')

# Hooks for hgweb permission checks; extensions can add hooks here.
# Each hook is invoked like this: hook(hgweb, request, operation),
# where operation is either read, pull or push. Hooks should either
# raise an ErrorResponse exception, or just return.
#
# It is possible to do both authentication and authorization through
# this.
permhooks = [checkauthz]


class ErrorResponse(Exception):
    def __init__(self, code, message=None, headers=None):
        if message is None:
            message = _statusmessage(code)
        Exception.__init__(self, message)
        self.code = code
        if headers is None:
            headers = []
        self.headers = headers

class continuereader(object):
    def __init__(self, f, write):
        self.f = f
        self._write = write
        self.continued = False

    def read(self, amt=-1):
        if not self.continued:
            self.continued = True
            self._write('HTTP/1.1 100 Continue\r\n\r\n')
        return self.f.read(amt)

    def __getattr__(self, attr):
        if attr in ('close', 'readline', 'readlines', '__iter__'):
            return getattr(self.f, attr)
        raise AttributeError

def _statusmessage(code):
    responses = httpserver.basehttprequesthandler.responses
    return responses.get(code, ('Error', 'Unknown error'))[0]

def statusmessage(code, message=None):
    return '%d %s' % (code, message or _statusmessage(code))

def get_stat(spath, fn):
    """stat fn if it exists, spath otherwise"""
    cl_path = os.path.join(spath, fn)
    if os.path.exists(cl_path):
        return os.stat(cl_path)
    else:
        return os.stat(spath)

def get_mtime(spath):
    return get_stat(spath, "00changelog.i").st_mtime

def ispathsafe(path):
    """Determine if a path is safe to use for filesystem access."""
    parts = path.split('/')
    for part in parts:
        if (part in ('', os.curdir, os.pardir) or
            pycompat.ossep in part or
            pycompat.osaltsep is not None and pycompat.osaltsep in part):
            return False

    return True

def staticfile(directory, fname, req):
    """return a file inside directory with guessed Content-Type header

    fname always uses '/' as directory separator and isn't allowed to
    contain unusual path components.
    Content-Type is guessed using the mimetypes module.
    Return an empty string if fname is illegal or file not found.

    """
    if not ispathsafe(fname):
        return

    fpath = os.path.join(*fname.split('/'))
    if isinstance(directory, str):
        directory = [directory]
    for d in directory:
        path = os.path.join(d, fpath)
        if os.path.exists(path):
            break
    try:
        os.stat(path)
        ct = mimetypes.guess_type(pycompat.fsdecode(path))[0] or "text/plain"
        with open(path, 'rb') as fh:
            data = fh.read()

        req.respond(HTTP_OK, ct, body=data)
    except TypeError:
        raise ErrorResponse(HTTP_SERVER_ERROR, 'illegal filename')
    except OSError as err:
        if err.errno == errno.ENOENT:
            raise ErrorResponse(HTTP_NOT_FOUND)
        else:
            raise ErrorResponse(HTTP_SERVER_ERROR,
                                encoding.strtolocal(err.strerror))

def paritygen(stripecount, offset=0):
    """count parity of horizontal stripes for easier reading"""
    if stripecount and offset:
        # account for offset, e.g. due to building the list in reverse
        count = (stripecount + offset) % stripecount
        parity = (stripecount + offset) / stripecount & 1
    else:
        count = 0
        parity = 0
    while True:
        yield parity
        count += 1
        if stripecount and count >= stripecount:
            parity = 1 - parity
            count = 0

def get_contact(config):
    """Return repo contact information or empty string.

    web.contact is the primary source, but if that is not set, try
    ui.username or $EMAIL as a fallback to display something useful.
    """
    return (config("web", "contact") or
            config("ui", "username") or
            encoding.environ.get("EMAIL") or "")

def caching(web, req):
    tag = r'W/"%d"' % web.mtime
    if req.env.get('HTTP_IF_NONE_MATCH') == tag:
        raise ErrorResponse(HTTP_NOT_MODIFIED)
    req.headers.append(('ETag', tag))

def cspvalues(ui):
    """Obtain the Content-Security-Policy header and nonce value.

    Returns a 2-tuple of the CSP header value and the nonce value.

    First value is ``None`` if CSP isn't enabled. Second value is ``None``
    if CSP isn't enabled or if the CSP header doesn't need a nonce.
    """
    # Without demandimport, "import uuid" could have an immediate side-effect
    # running "ldconfig" on Linux trying to find libuuid.
    # With Python <= 2.7.12, that "ldconfig" is run via a shell and the shell
    # may pollute the terminal with:
    #
    #   shell-init: error retrieving current directory: getcwd: cannot access
    #   parent directories: No such file or directory
    #
    # Python >= 2.7.13 has fixed it by running "ldconfig" directly without a
    # shell (hg changeset a09ae70f3489).
    #
    # Moved "import uuid" from here so it's executed after we know we have
    # a sane cwd (i.e. after dispatch.py cwd check).
    #
    # We can move it back once we no longer need Python <= 2.7.12 support.
    import uuid

    # Don't allow untrusted CSP setting since it be disable protections
    # from a trusted/global source.
    csp = ui.config('web', 'csp', untrusted=False)
    nonce = None

    if csp and '%nonce%' in csp:
        nonce = base64.urlsafe_b64encode(uuid.uuid4().bytes).rstrip('=')
        csp = csp.replace('%nonce%', nonce)

    return csp, nonce
