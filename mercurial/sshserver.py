# commands.py - command processing for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from i18n import gettext as _
from node import *
demandload(globals(), "sys util")

class sshserver(object):
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.lock = None
        self.fin = sys.stdin
        self.fout = sys.stdout

        sys.stdout = sys.stderr

        # Prevent insertion/deletion of CRs
        util.set_binary(self.fin)
        util.set_binary(self.fout)

    def getarg(self):
        argline = self.fin.readline()[:-1]
        arg, l = argline.split()
        val = self.fin.read(int(l))
        return arg, val

    def respond(self, v):
        self.fout.write("%d\n" % len(v))
        self.fout.write(v)
        self.fout.flush()

    def serve_forever(self):
        while self.serve_one(): pass
        sys.exit(0)

    def serve_one(self):
        cmd = self.fin.readline()[:-1]
        if cmd:
            impl = getattr(self, 'do_' + cmd, None)
            if impl: impl()
        return cmd != ''

    def do_heads(self):
        h = self.repo.heads()
        self.respond(" ".join(map(hex, h)) + "\n")

    def do_lock(self):
        self.lock = self.repo.lock()
        self.respond("")

    def do_unlock(self):
        if self.lock:
            self.lock.release()
        self.lock = None
        self.respond("")

    def do_branches(self):
        arg, nodes = self.getarg()
        nodes = map(bin, nodes.split(" "))
        r = []
        for b in self.repo.branches(nodes):
            r.append(" ".join(map(hex, b)) + "\n")
        self.respond("".join(r))

    def do_between(self):
        arg, pairs = self.getarg()
        pairs = [map(bin, p.split("-")) for p in pairs.split(" ")]
        r = []
        for b in self.repo.between(pairs):
            r.append(" ".join(map(hex, b)) + "\n")
        self.respond("".join(r))

    def do_changegroup(self):
        nodes = []
        arg, roots = self.getarg()
        nodes = map(bin, roots.split(" "))

        cg = self.repo.changegroup(nodes, 'serve')
        while True:
            d = cg.read(4096)
            if not d:
                break
            self.fout.write(d)

        self.fout.flush()

    def do_addchangegroup(self):
        if not self.lock:
            self.respond("not locked")
            return

        self.respond("")
        r = self.repo.addchangegroup(self.fin, 'serve')
        self.respond(str(r))
