#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import cStringIO, zlib, tempfile, errno, os, sys, urllib, copy
from mercurial import util, streamclone, pushkey
from mercurial.node import bin, hex
from mercurial import changegroup as changegroupmod
from common import ErrorResponse, HTTP_OK, HTTP_NOT_FOUND, HTTP_SERVER_ERROR

# __all__ is populated with the allowed commands. Be sure to add to it if
# you're adding a new command, or the new command won't work.

__all__ = [
   'lookup', 'heads', 'branches', 'between', 'changegroup',
   'changegroupsubset', 'capabilities', 'unbundle', 'stream_out',
   'branchmap', 'pushkey', 'listkeys'
]

HGTYPE = 'application/mercurial-0.1'
basecaps = 'lookup changegroupsubset branchmap pushkey'.split()

def capabilities(repo, req):
    caps = copy.copy(basecaps)
    if streamclone.allowed(repo.ui):
        caps.append('stream=%d' % repo.changelog.version)
    if changegroupmod.bundlepriority:
        caps.append('unbundle=%s' % ','.join(changegroupmod.bundlepriority))
    rsp = ' '.join(caps)
    req.respond(HTTP_OK, HGTYPE, length=len(rsp))
    yield rsp

def unbundle(repo, req):

    proto = req.env.get('wsgi.url_scheme') or 'http'
    their_heads = req.form['heads'][0].split(' ')

    def check_heads():
        heads = map(hex, repo.heads())
        return their_heads == [hex('force')] or their_heads == heads

    # fail early if possible
    if not check_heads():
        req.drain()
        raise ErrorResponse(HTTP_OK, 'unsynced changes')

    # do not lock repo until all changegroup data is
    # streamed. save to temporary file.

    fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
    fp = os.fdopen(fd, 'wb+')
    try:
        length = int(req.env['CONTENT_LENGTH'])
        for s in util.filechunkiter(req, limit=length):
            fp.write(s)

        try:
            lock = repo.lock()
            try:
                if not check_heads():
                    raise ErrorResponse(HTTP_OK, 'unsynced changes')

                fp.seek(0)
                header = fp.read(6)
                if header.startswith('HG') and not header.startswith('HG10'):
                    raise ValueError('unknown bundle version')
                elif header not in changegroupmod.bundletypes:
                    raise ValueError('unknown bundle compression type')
                gen = changegroupmod.unbundle(header, fp)

                # send addchangegroup output to client

                oldio = sys.stdout, sys.stderr
                sys.stderr = sys.stdout = cStringIO.StringIO()

                try:
                    url = 'remote:%s:%s:%s' % (
                          proto,
                          urllib.quote(req.env.get('REMOTE_HOST', '')),
                          urllib.quote(req.env.get('REMOTE_USER', '')))
                    try:
                        ret = repo.addchangegroup(gen, 'serve', url, lock=lock)
                    except util.Abort, inst:
                        sys.stdout.write("abort: %s\n" % inst)
                        ret = 0
                finally:
                    val = sys.stdout.getvalue()
                    sys.stdout, sys.stderr = oldio
                req.respond(HTTP_OK, HGTYPE)
                return '%d\n%s' % (ret, val),
            finally:
                lock.release()
        except ValueError, inst:
            raise ErrorResponse(HTTP_OK, inst)
        except (OSError, IOError), inst:
            error = getattr(inst, 'strerror', 'Unknown error')
            if not isinstance(error, str):
                error = 'Error: %s' % str(error)
            if inst.errno == errno.ENOENT:
                code = HTTP_NOT_FOUND
            else:
                code = HTTP_SERVER_ERROR
            filename = getattr(inst, 'filename', '')
            # Don't send our filesystem layout to the client
            if filename and filename.startswith(repo.root):
                filename = filename[len(repo.root)+1:]
                text = '%s: %s' % (error, filename)
            else:
                text = error.replace(repo.root + os.path.sep, '')
            raise ErrorResponse(code, text)
    finally:
        fp.close()
        os.unlink(tempname)
