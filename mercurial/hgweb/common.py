# hgweb/common.py - Utility functions needed by hgweb_mod and hgwebdir_mod
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import errno, mimetypes, os

HTTP_OK = 200
HTTP_BAD_REQUEST = 400
HTTP_UNAUTHORIZED = 401
HTTP_FORBIDDEN = 403
HTTP_NOT_FOUND = 404
HTTP_METHOD_NOT_ALLOWED = 405
HTTP_SERVER_ERROR = 500

class ErrorResponse(Exception):
    def __init__(self, code, message=None, headers=[]):
        Exception.__init__(self)
        self.code = code
        self.headers = headers
        if message is not None:
            self.message = message
        else:
            self.message = _statusmessage(code)

def _statusmessage(code):
    from BaseHTTPServer import BaseHTTPRequestHandler
    responses = BaseHTTPRequestHandler.responses
    return responses.get(code, ('Error', 'Unknown error'))[0]

def statusmessage(code):
    return '%d %s' % (code, _statusmessage(code))

def get_mtime(repo_path):
    store_path = os.path.join(repo_path, ".hg")
    if not os.path.isdir(os.path.join(store_path, "data")):
        store_path = os.path.join(store_path, "store")
    cl_path = os.path.join(store_path, "00changelog.i")
    if os.path.exists(cl_path):
        return os.stat(cl_path).st_mtime
    else:
        return os.stat(store_path).st_mtime

def staticfile(directory, fname, req):
    """return a file inside directory with guessed Content-Type header

    fname always uses '/' as directory separator and isn't allowed to
    contain unusual path components.
    Content-Type is guessed using the mimetypes module.
    Return an empty string if fname is illegal or file not found.

    """
    parts = fname.split('/')
    for part in parts:
        if (part in ('', os.curdir, os.pardir) or
            os.sep in part or os.altsep is not None and os.altsep in part):
            return ""
    fpath = os.path.join(*parts)
    if isinstance(directory, str):
        directory = [directory]
    for d in directory:
        path = os.path.join(d, fpath)
        if os.path.exists(path):
            break
    try:
        os.stat(path)
        ct = mimetypes.guess_type(path)[0] or "text/plain"
        req.respond(HTTP_OK, ct, length = os.path.getsize(path))
        return open(path, 'rb').read()
    except TypeError:
        raise ErrorResponse(HTTP_SERVER_ERROR, 'illegal filename')
    except OSError, err:
        if err.errno == errno.ENOENT:
            raise ErrorResponse(HTTP_NOT_FOUND)
        else:
            raise ErrorResponse(HTTP_SERVER_ERROR, err.strerror)

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
            os.environ.get("EMAIL") or "")
