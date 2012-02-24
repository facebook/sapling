# Copyright 2011 Fog Creek Software
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import urllib2

from mercurial import error, httprepo, util, wireproto
from mercurial.i18n import _

import lfutil

LARGEFILES_REQUIRED_MSG = ('\nThis repository uses the largefiles extension.'
                           '\n\nPlease enable it in your Mercurial config '
                           'file.\n')

def putlfile(repo, proto, sha):
    '''Put a largefile into a repository's local store and into the
    user cache.'''
    proto.redirect()

    tmpfp = util.atomictempfile(lfutil.storepath(repo, sha),
                                createmode=repo.store.createmode)
    try:
        try:
            proto.getfile(tmpfp)
            tmpfp._fp.seek(0)
            if sha != lfutil.hexsha1(tmpfp._fp):
                raise IOError(0, _('largefile contents do not match hash'))
            tmpfp.close()
            lfutil.linktousercache(repo, sha)
        except IOError, e:
            repo.ui.warn(_('largefiles: failed to put %s into store: %s') %
                         (sha, e.strerror))
            return wireproto.pushres(1)
    finally:
        tmpfp.discard()

    return wireproto.pushres(0)

def getlfile(repo, proto, sha):
    '''Retrieve a largefile from the repository-local cache or system
    cache.'''
    filename = lfutil.findfile(repo, sha)
    if not filename:
        raise util.Abort(_('requested largefile %s not present in cache') % sha)
    f = open(filename, 'rb')
    length = os.fstat(f.fileno())[6]

    # Since we can't set an HTTP content-length header here, and
    # Mercurial core provides no way to give the length of a streamres
    # (and reading the entire file into RAM would be ill-advised), we
    # just send the length on the first line of the response, like the
    # ssh proto does for string responses.
    def generator():
        yield '%d\n' % length
        for chunk in f:
            yield chunk
    return wireproto.streamres(generator())

def statlfile(repo, proto, sha):
    '''Return '2\n' if the largefile is missing, '1\n' if it has a
    mismatched checksum, or '0\n' if it is in good condition'''
    filename = lfutil.findfile(repo, sha)
    if not filename:
        return '2\n'
    fd = None
    try:
        fd = open(filename, 'rb')
        return lfutil.hexsha1(fd) == sha and '0\n' or '1\n'
    finally:
        if fd:
            fd.close()

def wirereposetup(ui, repo):
    class lfileswirerepository(repo.__class__):
        def putlfile(self, sha, fd):
            # unfortunately, httprepository._callpush tries to convert its
            # input file-like into a bundle before sending it, so we can't use
            # it ...
            if issubclass(self.__class__, httprepo.httprepository):
                res = None
                try:
                    res = self._call('putlfile', data=fd, sha=sha,
                        headers={'content-type':'application/mercurial-0.1'})
                    d, output = res.split('\n', 1)
                    for l in output.splitlines(True):
                        self.ui.warn(_('remote: '), l, '\n')
                    return int(d)
                except (ValueError, urllib2.HTTPError):
                    self.ui.warn(_('unexpected putlfile response: %s') % res)
                    return 1
            # ... but we can't use sshrepository._call because the data=
            # argument won't get sent, and _callpush does exactly what we want
            # in this case: send the data straight through
            else:
                try:
                    ret, output = self._callpush("putlfile", fd, sha=sha)
                    if ret == "":
                        raise error.ResponseError(_('putlfile failed:'),
                                output)
                    return int(ret)
                except IOError:
                    return 1
                except ValueError:
                    raise error.ResponseError(
                        _('putlfile failed (unexpected response):'), ret)

        def getlfile(self, sha):
            stream = self._callstream("getlfile", sha=sha)
            length = stream.readline()
            try:
                length = int(length)
            except ValueError:
                self._abort(error.ResponseError(_("unexpected response:"),
                                                length))
            return (length, stream)

        def statlfile(self, sha):
            try:
                return int(self._call("statlfile", sha=sha))
            except (ValueError, urllib2.HTTPError):
                # If the server returns anything but an integer followed by a
                # newline, newline, it's not speaking our language; if we get
                # an HTTP error, we can't be sure the largefile is present;
                # either way, consider it missing.
                return 2

    repo.__class__ = lfileswirerepository

# advertise the largefiles=serve capability
def capabilities(repo, proto):
    return capabilities_orig(repo, proto) + ' largefiles=serve'

# duplicate what Mercurial's new out-of-band errors mechanism does, because
# clients old and new alike both handle it well
def webproto_refuseclient(self, message):
    self.req.header([('Content-Type', 'application/hg-error')])
    return message

def sshproto_refuseclient(self, message):
    self.ui.write_err('%s\n-\n' % message)
    self.fout.write('\n')
    self.fout.flush()

    return ''

def heads(repo, proto):
    if lfutil.islfilesrepo(repo):
        return wireproto.ooberror(LARGEFILES_REQUIRED_MSG)
    return wireproto.heads(repo, proto)

def sshrepo_callstream(self, cmd, **args):
    if cmd == 'heads' and self.capable('largefiles'):
        cmd = 'lheads'
    if cmd == 'batch' and self.capable('largefiles'):
        args['cmds'] = args['cmds'].replace('heads ', 'lheads ')
    return ssh_oldcallstream(self, cmd, **args)

def httprepo_callstream(self, cmd, **args):
    if cmd == 'heads' and self.capable('largefiles'):
        cmd = 'lheads'
    if cmd == 'batch' and self.capable('largefiles'):
        args['cmds'] = args['cmds'].replace('heads ', 'lheads ')
    return http_oldcallstream(self, cmd, **args)
