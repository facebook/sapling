# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sshserver.py - ssh protocol server support for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import os
import socket
import sys

from . import encoding, error, hook, util, wireproto
from .i18n import _


class sshserver(wireproto.abstractserverproto):
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.lock = None
        self.fin = ui.fin
        self.fout = ui.fout
        self.name = "ssh"

        ui.fout = repo.ui.fout = ui.ferr

        # Prevent insertion/deletion of CRs
        util.setbinary(self.fin)
        util.setbinary(self.fout)

    def getargs(self, args):
        data = {}
        keys = args.split()
        for n in range(len(keys)):
            argline = self.fin.readline()[:-1].decode()
            arg, l = argline.split()
            if arg not in keys:
                raise error.Abort(_("unexpected parameter %r") % arg)
            if arg == "*":
                star = {}
                for k in range(int(l)):
                    argline = self.fin.readline()[:-1].decode()
                    arg, l = argline.split()
                    val = self.fin.read(int(l)).decode()
                    star[arg] = val
                data["*"] = star
            else:
                val = self.fin.read(int(l)).decode()
                data[arg] = val
        return [data[k] for k in keys]

    def getarg(self, name):
        return self.getargs(name)[0]

    def getfile(self, fpout):
        self.sendresponse("")
        count = int(self.fin.readline())
        while count:
            fpout.write(self.fin.read(count))
            count = int(self.fin.readline())

    def redirect(self):
        pass

    def sendresponse(self, v):
        self.sendbytesresponse(v.encode())

    def sendbytesresponse(self, v):
        self.fout.write(b"%d\n" % len(v))
        self.fout.write(v)
        self.fout.flush()

    def sendstream(self, source):
        write = self.fout.write

        if source.reader:
            gen = iter(lambda: source.reader.read(4096), "")
        else:
            gen = source.gen

        for chunk in gen:
            write(chunk)
        self.fout.flush()

    def sendpushresponse(self, rsp):
        self.sendresponse("")
        self.sendresponse(str(rsp.res))

    def sendpusherror(self, rsp):
        self.sendresponse(rsp.res)

    def sendooberror(self, rsp):
        self.ui.ferr.write("%s\n-\n" % rsp.message)
        self.ui.ferr.flush()
        self.fout.write(b"\n")
        self.fout.flush()

    def serve_forever(self):
        try:
            while self.serve_one():
                pass
        finally:
            if self.lock is not None:
                self.lock.release()
        sys.exit(0)

    handlers = {
        bytes: sendbytesresponse,
        str: sendresponse,
        wireproto.streamres: sendstream,
        wireproto.pushres: sendpushresponse,
        wireproto.pusherr: sendpusherror,
        wireproto.ooberror: sendooberror,
    }

    def serve_one(self):
        cmd = self.fin.readline()[:-1]
        cmd = cmd.decode()
        if cmd:
            if hasattr(util, "setprocname"):
                client = encoding.environ.get("SSH_CLIENT", "").split(" ")[0]
                # Resolve IP to hostname
                try:
                    client = socket.gethostbyaddr(client)[0]
                except (socket.error, IndexError):
                    pass
                reponame = os.path.basename(self.repo.root)
                title = "hg serve (%s)" % " ".join(
                    filter(None, [reponame, cmd, client])
                )
                util.setprocname(title)
            if cmd in wireproto.commands:
                rsp = wireproto.dispatch(self.repo, self, cmd)
                self.handlers[rsp.__class__](self, rsp)
            else:
                impl = getattr(self, "do_" + cmd, None)
                if impl:
                    r = impl()
                    if r is not None:
                        self.sendresponse(r)
                else:
                    self.sendresponse("")
        return cmd != ""

    def _client(self):
        client = encoding.environ.get("SSH_CLIENT", "").split(" ", 1)[0]
        return "remote:ssh:" + client
