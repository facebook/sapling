# hgweb/common.py - Utility functions needed by hgweb_mod and hgwebdir_mod
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import errno, mimetypes, os

class ErrorResponse(Exception):
    def __init__(self, code, message=None):
        Exception.__init__(self)
        self.code = code
        if message:
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
    """return a file inside directory with guessed content-type header

    fname always uses '/' as directory separator and isn't allowed to
    contain unusual path components.
    Content-type is guessed using the mimetypes module.
    Return an empty string if fname is illegal or file not found.

    """
    parts = fname.split('/')
    path = directory
    for part in parts:
        if (part in ('', os.curdir, os.pardir) or
            os.sep in part or os.altsep is not None and os.altsep in part):
            return ""
        path = os.path.join(path, part)
    try:
        os.stat(path)
        ct = mimetypes.guess_type(path)[0] or "text/plain"
        req.header([('Content-type', ct),
                    ('Content-length', str(os.path.getsize(path)))])
        return file(path, 'rb').read()
    except TypeError:
        raise ErrorResponse(500, 'illegal file name')
    except OSError, err:
        if err.errno == errno.ENOENT:
            raise ErrorResponse(404)
        else:
            raise ErrorResponse(500, err.strerror)

def style_map(templatepath, style):
    """Return path to mapfile for a given style.

    Searches mapfile in the following locations:
    1. templatepath/style/map
    2. templatepath/map-style
    3. templatepath/map
    """
    locations = style and [os.path.join(style, "map"), "map-"+style] or []
    locations.append("map")
    for location in locations:
        mapfile = os.path.join(templatepath, location)
        if os.path.isfile(mapfile):
            return mapfile
    raise RuntimeError("No hgweb templates found in %r" % templatepath)

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
