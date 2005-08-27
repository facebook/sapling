# sshrepo.py - ssh repository proxy class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, re, select
from node import *
from remoterepo import *

class sshrepository(remoterepository):
    def __init__(self, ui, path):
        self.url = path
        self.ui = ui

        m = re.match(r'ssh://(([^@]+)@)?([^:/]+)(:(\d+))?(/(.*))?', path)
        if not m:
            raise RepoError("couldn't parse destination %s" % path)

        self.user = m.group(2)
        self.host = m.group(3)
        self.port = m.group(5)
        self.path = m.group(7) or "."

        args = self.user and ("%s@%s" % (self.user, self.host)) or self.host
        args = self.port and ("%s -p %s") % (args, self.port) or args

        sshcmd = self.ui.config("ui", "ssh", "ssh")
        remotecmd = self.ui.config("ui", "remotecmd", "hg")
        cmd = "%s %s '%s -R %s serve --stdio'"
        cmd = cmd % (sshcmd, args, remotecmd, self.path)

        self.pipeo, self.pipei, self.pipee = os.popen3(cmd)

    def readerr(self):
        while 1:
            r,w,x = select.select([self.pipee], [], [], 0)
            if not r: break
            l = self.pipee.readline()
            if not l: break
            self.ui.status("remote: ", l)

    def __del__(self):
        try:
            self.pipeo.close()
            self.pipei.close()
            for l in self.pipee:
                self.ui.status("remote: ", l)
            self.pipee.close()
        except:
            pass

    def dev(self):
        return -1

    def do_cmd(self, cmd, **args):
        self.ui.debug("sending %s command\n" % cmd)
        self.pipeo.write("%s\n" % cmd)
        for k, v in args.items():
            self.pipeo.write("%s %d\n" % (k, len(v)))
            self.pipeo.write(v)
        self.pipeo.flush()

        return self.pipei

    def call(self, cmd, **args):
        r = self.do_cmd(cmd, **args)
        l = r.readline()
        self.readerr()
        try:
            l = int(l)
        except:
            raise RepoError("unexpected response '%s'" % l)
        return r.read(l)

    def lock(self):
        self.call("lock")
        return remotelock(self)

    def unlock(self):
        self.call("unlock")

    def heads(self):
        d = self.call("heads")
        try:
            return map(bin, d[:-1].split(" "))
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def branches(self, nodes):
        n = " ".join(map(hex, nodes))
        d = self.call("branches", nodes=n)
        try:
            br = [ tuple(map(bin, b.split(" "))) for b in d.splitlines() ]
            return br
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def between(self, pairs):
        n = "\n".join(["-".join(map(hex, p)) for p in pairs])
        d = self.call("between", pairs=n)
        try:
            p = [ l and map(bin, l.split(" ")) or [] for l in d.splitlines() ]
            return p
        except:
            raise RepoError("unexpected response '%s'" % (d[:400] + "..."))

    def changegroup(self, nodes):
        n = " ".join(map(hex, nodes))
        f = self.do_cmd("changegroup", roots=n)
        return self.pipei

    def addchangegroup(self, cg):
        d = self.call("addchangegroup")
        if d:
            raise RepoError("push refused: %s", d)

        while 1:
            d = cg.read(4096)
            if not d: break
            self.pipeo.write(d)
            self.readerr()

        self.pipeo.flush()

        self.readerr()
        l = int(self.pipei.readline())
        return self.pipei.read(l) != ""
