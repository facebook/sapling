# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""test network connections to the server

::

    [debugnetwork]
    # Define the location of the speed test command on the server.
    speed-test-command = /path/to/speed-test

    # Set how many bytes to download in the download test.
    speed-test-download-size = 10M

    # Set how many bytes to upload in the upload test.
    speed-test-upload-size = 1M
"""

import os
import socket

from edenscm.mercurial import (
    cmdutil,
    error,
    hg,
    progress,
    pycompat,
    registrar,
    sshpeer,
    util,
)
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

SOCKET_AF = {
    getattr(socket, field): field for field in dir(socket) if field.startswith("AF_")
}
BLOCK_SIZE = 2 * 1024 * 1024


def checkdnsresolution(ui, url):
    ui.status(_("Resolving remote hostname: %s\n") % url.host, component="debugnetwork")
    try:
        addrinfos = socket.getaddrinfo(url.host, url.port or 22, 0, socket.SOCK_STREAM)
    except Exception as e:
        ui.status(_("failed to resolve hostname: %s\n") % e, error=_("error"))
        return None

    for addrinfo in addrinfos:
        family = SOCKET_AF.get(addrinfo[0], addrinfo[0])
        addr = addrinfo[4][0]
        ui.status(_("Resolved: %s %s\n") % (family, addr), component="debugnetwork")

    return addrinfos


def checkreachability(ui, url, addrinfos):
    ok = False
    for family, socktype, _proto, _canonname, sockaddr in addrinfos:
        ui.status(
            _("Testing connection to: %s %s\n") % (sockaddr[0], sockaddr[1]),
            component="debugnetwork",
        )
        try:
            starttime = util.timer()
            s = socket.socket(family, socktype)
            s.settimeout(1)
            s.connect(sockaddr)
            s.shutdown(socket.SHUT_RDWR)
            endtime = util.timer()
            ui.status(
                _("Connected ok: %s\n") % util.timecount(endtime - starttime),
                component="debugnetwork",
            )
            ok = True
        except Exception as e:
            ui.status(_("failed to connect to remote host: %s\n") % e, error=_("error"))
    return ok


def checksshcommand(ui, url, opts):
    rui = hg.remoteui(ui, opts)
    sshcmd = rui.config("ui", "ssh")
    sshaddenv = dict(rui.configitems("sshenv"))
    sshenv = util.shellenviron(sshaddenv)
    args = util.sshargs(sshcmd, url.host, url.user, url.port)
    cmd = "%s %s %s" % (sshcmd, args, "hostname")
    ui.status(
        _("Testing SSH connection to the server: running 'hostname'\n"),
        component="debugnetwork",
    )
    ui.pushbuffer(subproc=True)
    starttime = util.timer()
    res = ui.system(cmd, blockedtag="debugnetwork", environ=sshenv)
    endtime = util.timer()
    hostname = pycompat.decodeutf8(ui.popbufferbytes()).strip()
    if res == 0:
        ui.status(
            _("Connected ok: %s\n") % util.timecount(endtime - starttime),
            component="debugnetwork",
        )
        ui.status(_("Server hostname is %s\n") % hostname, component="debugnetwork")
        return True
    else:
        ui.status(_("Failed to connect: ssh returned %s\n") % res, error=_("error"))
        return False


def checkhgserver(ui, repo, opts, path):
    ui.status(
        _("Testing connection to Mercurial on the server: querying master bookmark\n"),
        component="debugnetwork",
    )
    starttime = util.timer()
    peer = None
    try:
        peer = hg.peer(repo, opts, path)
        bookmarks = peer.listkeys("bookmarks")
        master = bookmarks.get("master")
    except Exception as e:
        ui.status(_("failed to connect to Mercurial: %s\n") % e, error=_("error"))
        return False
    finally:
        if peer:
            peer.close()
    endtime = util.timer()

    ui.status(
        _("Connected ok: %s\n") % util.timecount(endtime - starttime),
        component="debugnetwork",
    )
    if master:
        ui.status(
            _("Server master bookmark is %s\n") % master, component="debugnetwork"
        )
    else:
        ui.status(_("Server has no master bookmark\n"), component="debugnetwork")
    return True


def checkhgspeed(ui, url, opts):
    speedcmd = ui.config("debugnetwork", "speed-test-command")
    if speedcmd is None:
        ui.status(
            _(
                "Not testing connection speed: 'debugnetwork.speed-test-command' is not set"
            ),
            component="debugnetwork",
        )
        return True
    ui.status(_("Testing connection speed to the server\n"), component="debugnetwork")
    rui = hg.remoteui(ui, opts)
    sshcmd = rui.config("ui", "ssh")
    sshaddenv = dict(rui.configitems("sshenv"))
    sshenv = util.shellenviron(sshaddenv)
    args = util.sshargs(sshcmd, url.host, url.user, url.port)
    download = ui.configbytes("debugnetwork", "speed-test-download-size", 10000000)
    upload = ui.configbytes("debugnetwork", "speed-test-upload-size", 1000000)

    cmd = "%s %s %s" % (sshcmd, args, util.shellquote(sshpeer._serverquote(speedcmd)))
    pipeo, pipei, pipee, sub = util.popen4(cmd, bufsize=0, env=sshenv)
    pipee = sshpeer.threadedstderr(rui, pipee)
    pipee.start()

    def latencytest(count):
        # Use the upload endpoint for the latency test.  We will time how long it
        # takes for the server to return the "upload complete" response for a
        # single byte upload.
        latencies = []
        with progress.spinner(ui, "testing connection latency"):
            for i in range(count):
                pipeo.write(b"upload 1\n")
                pipeo.flush()
                l = pipei.readline()
                if l != b"upload bytes 1\n":
                    raise error.Abort("invalid response from server: %r" % l)
                starttime = util.timer()
                pipeo.write(b"\n")
                pipeo.flush()
                l = pipei.readline()
                endtime = util.timer()
                if l != b"upload complete\n":
                    raise error.Abort("invalid response from server: %r" % l)
                latencies.append(endtime - starttime)
        return latencies

    def downloadtest(description, bytecount):
        pipeo.write(b"download %i\n" % bytecount)
        pipeo.flush()
        l = pipei.readline()
        if not l or not l.startswith(b"download bytes"):
            raise error.Abort("invalid response from server: %r" % l)
        bytecount = int(l.split()[2])
        with progress.bar(
            ui, description, total=bytecount, formatfunc=util.bytecount
        ) as prog:
            starttime = util.timer()
            remaining = bytecount
            while remaining > 0:
                data = pipei.read(min(remaining, BLOCK_SIZE))
                if not data:
                    raise error.Abort("premature end of speed-test download stream")
                remaining -= len(data)
                prog.value = bytecount - remaining
            l = pipei.readline()
            if not l or not l.startswith(b"download complete"):
                raise error.Abort("invalid response from server: %r" % l)
            endtime = util.timer()
        return endtime - starttime

    def uploadtest(description, bytecount):
        pipeo.write(b"upload %i\n" % bytecount)
        pipeo.flush()
        l = pipei.readline()
        if not l or not l.startswith(b"upload bytes"):
            raise error.Abort("invalid response from server: %r" % l)
        bytecount = int(l.split()[2])
        with progress.bar(
            ui, description, total=bytecount, formatfunc=util.bytecount
        ) as prog:
            starttime = util.timer()
            remaining = bytecount
            while remaining > 0:
                data = os.urandom(min(remaining, BLOCK_SIZE))
                remaining -= len(data)
                pipeo.write(data)
                prog.value = bytecount - remaining
            pipeo.flush()
            l = pipei.readline()
            if not l or not l.startswith(b"upload complete"):
                raise error.Abort("invalid response from server: %r" % l)
            endtime = util.timer()
        return endtime - starttime

    def printresult(testname, bytecount, testtime):
        byterate = bytecount / testtime
        ui.status(
            _("Speed: %s %s in %s (%0.2f Mbit/s, %0.2f MiB/s)\n")
            % (
                testname,
                util.bytecount(bytecount),
                util.timecount(testtime),
                8 * byterate / 1000000,
                byterate / (1024 * 1024),
            ),
            component="debugnetwork",
        )

    try:
        latencies = latencytest(5)
        latency = sum(latencies, 0) / len(latencies)
        ui.status(
            _("Latency: %s (average of %s round-trips)\n")
            % (util.timecount(latency), len(latencies)),
            component="debugnetwork",
        )

        for testfunc, testname, bytecount in [
            (downloadtest, "download", download),
            (uploadtest, "upload", upload),
        ]:
            warmuptime = testfunc("warming up for %s test" % testname, bytecount)
            if warmuptime < 0.2:
                # The network is sufficiently fast that we warmed up in <200ms.
                # To make the test more meaningful, increase the size of data
                # 25x (which should give a maximum test time of 5s).
                bytecount *= 25
                warmuptime = testfunc(
                    "warming up for large %s test" % testname, bytecount
                )
            printresult("(round 1) %sed" % testname, bytecount, warmuptime)
            testtime = testfunc(testname, bytecount)
            printresult("(round 2) %sed" % testname, bytecount, testtime)
        return True
    except Exception as e:
        ui.warn(_("error testing speed: %s\n") % e)
        return False


@command(
    "debugnetwork",
    [
        ("", "connection", False, _("run connection tests")),
        ("", "speed", False, _("run speed tests")),
    ]
    + cmdutil.remoteopts,
    _("[REMOTE]"),
)
def debugnetwork(ui, repo, remote="default", **opts):
    """debug the network connection to a remote"""

    alltests = not any(opts.get(opt) for opt in ["connection", "speed"])

    ui.status(_("Remote name: %s\n") % remote, component="debugnetwork")

    path, branches = hg.parseurl(repo.ui.expandpath(remote))
    ui.status(_("Remote url: %s\n") % path, component="debugnetwork")

    url = util.url(path)
    if url.scheme != "ssh":
        ui.status(_("Not checking network as remote is not an ssh peer"))
        return 1

    if alltests or opts.get("connection"):
        addrinfos = checkdnsresolution(ui, url)
        if not addrinfos:
            msg = _("Failed to look-up the server in DNS.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 2

        if not checkreachability(ui, url, addrinfos):
            msg = _("Failed to connect to the server on any address.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 3

        if not checksshcommand(ui, url, opts):
            msg = _("Failed to connect to SSH on the server.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 4

        if not checkhgserver(ui, repo, opts, path):
            msg = _("Failed to connect to Mercurial on the server.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 5

    if alltests or opts.get("speed"):
        if not checkhgspeed(ui, url, opts):
            msg = _("Failed to check Mercurial server connection speed.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 6
