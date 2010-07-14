# sshserver.py - ssh protocol server support for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import bin, hex
import streamclone, util, hook, pushkey, wireproto
import os, sys, tempfile, urllib, copy

class sshserver(object):

    caps = 'unbundle lookup changegroupsubset branchmap pushkey'.split()

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.lock = None
        self.fin = sys.stdin
        self.fout = sys.stdout

        hook.redirect(True)
        sys.stdout = sys.stderr

        # Prevent insertion/deletion of CRs
        util.set_binary(self.fin)
        util.set_binary(self.fout)

    def getargs(self, args):
        data = {}
        keys = args.split()
        count = len(keys)
        for n in xrange(len(keys)):
            argline = self.fin.readline()[:-1]
            arg, l = argline.split()
            val = self.fin.read(int(l))
            if arg not in keys:
                raise util.Abort("unexpected parameter %r" % arg)
            if arg == '*':
                star = {}
                for n in xrange(int(l)):
                    arg, l = argline.split()
                    val = self.fin.read(int(l))
                    star[arg] = val
                data['*'] = star
            else:
                data[arg] = val
        return [data[k] for k in keys]

    def getarg(self, name):
        return self.getargs(name)[0]

    def respond(self, v):
        self.fout.write("%d\n" % len(v))
        self.fout.write(v)
        self.fout.flush()

    def sendchangegroup(self, changegroup):
        while True:
            d = changegroup.read(4096)
            if not d:
                break
            self.fout.write(d)

        self.fout.flush()

    def serve_forever(self):
        try:
            while self.serve_one():
                pass
        finally:
            if self.lock is not None:
                self.lock.release()
        sys.exit(0)

    def serve_one(self):
        cmd = self.fin.readline()[:-1]
        if cmd and not wireproto.dispatch(self.repo, self, cmd):
            impl = getattr(self, 'do_' + cmd, None)
            if impl:
                r = impl()
                if r is not None:
                    self.respond(r)
            else: self.respond("")
        return cmd != ''

    def do_hello(self):
        '''the hello command returns a set of lines describing various
        interesting things about the server, in an RFC822-like format.
        Currently the only one defined is "capabilities", which
        consists of a line in the form:

        capabilities: space separated list of tokens
        '''
        caps = copy.copy(self.caps)
        if streamclone.allowed(self.repo.ui):
            caps.append('stream=%d' % self.repo.changelog.version)
        return "capabilities: %s\n" % (' '.join(caps),)

    def do_lock(self):
        '''DEPRECATED - allowing remote client to lock repo is not safe'''

        self.lock = self.repo.lock()
        return ""

    def do_unlock(self):
        '''DEPRECATED'''

        if self.lock:
            self.lock.release()
        self.lock = None
        return ""

    def do_addchangegroup(self):
        '''DEPRECATED'''

        if not self.lock:
            self.respond("not locked")
            return

        self.respond("")
        r = self.repo.addchangegroup(self.fin, 'serve', self.client_url(),
                                     lock=self.lock)
        return str(r)

    def client_url(self):
        client = os.environ.get('SSH_CLIENT', '').split(' ', 1)[0]
        return 'remote:ssh:' + client

    def do_unbundle(self):
        their_heads = self.getarg('heads').split()

        def check_heads():
            heads = map(hex, self.repo.heads())
            return their_heads == [hex('force')] or their_heads == heads

        # fail early if possible
        if not check_heads():
            self.respond(_('unsynced changes'))
            return

        self.respond('')

        # write bundle data to temporary file because it can be big
        fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
        fp = os.fdopen(fd, 'wb+')
        try:
            count = int(self.fin.readline())
            while count:
                fp.write(self.fin.read(count))
                count = int(self.fin.readline())

            was_locked = self.lock is not None
            if not was_locked:
                self.lock = self.repo.lock()
            try:
                if not check_heads():
                    # someone else committed/pushed/unbundled while we
                    # were transferring data
                    self.respond(_('unsynced changes'))
                    return
                self.respond('')

                # push can proceed

                fp.seek(0)
                r = self.repo.addchangegroup(fp, 'serve', self.client_url(),
                                             lock=self.lock)
                self.respond(str(r))
            finally:
                if not was_locked:
                    self.lock.release()
                    self.lock = None
        finally:
            fp.close()
            os.unlink(tempname)

    def do_stream_out(self):
        try:
            for chunk in streamclone.stream_out(self.repo):
                self.fout.write(chunk)
            self.fout.flush()
        except streamclone.StreamException, inst:
            self.fout.write(str(inst))
            self.fout.flush()
