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

from edenscm.mercurial import cmdutil, hg, progress, registrar, sshpeer, util
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
                _("Connected ok: %0.3f seconds\n") % (endtime - starttime),
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
    starttime = util.timer()
    res = ui.system(cmd, blockedtag="debugnetwork", environ=sshenv)
    endtime = util.timer()
    if res == 0:
        ui.status(
            _("Connected ok: %0.3f seconds\n") % (endtime - starttime),
            component="debugnetwork",
        )
        return True
    else:
        ui.status(_("Failed to connect: ssh returned %s\n") % res, error=_("error"))
        return False


def checkhgserver(ui, repo, opts, path):
    ui.status(
        _("Testing connection to Mercurial on the server: querying master bookmark\n"),
        component="debugnetwork",
    )
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

    if master:
        ui.status(
            _("Connected ok: server master bookmark is %s\n") % master,
            component="debugnetwork",
        )
    else:
        ui.status(
            _("Connected ok: server has no master bookmark\n"), component="debugnetwork"
        )
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

    cmd = "%s %s %s" % (
        sshcmd,
        args,
        util.shellquote(
            "%s %s %s" % (sshpeer._serverquote(speedcmd), download, upload)
        ),
    )
    pipeo, pipei, pipee, sub = util.popen4(cmd, bufsize=0, env=sshenv)
    pipee = sshpeer.threadedstderr(rui, pipee)
    pipee.start()
    while True:
        l = pipei.readline()
        if not l:
            break
        if l.startswith("download bytes"):
            starttime = util.timer()
            bytecount = int(l.split()[2])
            with progress.bar(
                ui, "testing download speed", total=bytecount, formatfunc=util.bytecount
            ) as prog:
                remaining = bytecount
                while remaining > 0:
                    data = pipei.read(min(remaining, BLOCK_SIZE))
                    remaining -= len(data)
                    prog.value = bytecount - remaining
            l = pipei.readline()
            if not l.startswith("download complete"):
                return False
            endtime = util.timer()
            byterate = bytecount / (endtime - starttime)
            ui.status(
                _("Downloaded %s bytes in %0.3f seconds (%0.2f Mbit/s, %0.2f MiB/s)\n")
                % (
                    bytecount,
                    endtime - starttime,
                    8 * byterate / 1000000,
                    byterate / (1024 * 1024),
                ),
                component="debugnetwork",
            )
        if l.startswith("upload bytes"):
            starttime = util.timer()
            bytecount = int(l.split()[2])
            with progress.bar(
                ui, "testing upload speed", total=bytecount, formatfunc=util.bytecount
            ) as prog:
                remaining = bytecount
                while remaining > 0:
                    data = os.urandom(min(remaining, BLOCK_SIZE))
                    remaining -= len(data)
                    pipeo.write(data)
                    prog.value = bytecount - remaining
                pipeo.flush()
            l = pipei.readline()
            if not l.startswith("upload complete"):
                return False
            endtime = util.timer()
            byterate = bytecount / (endtime - starttime)
            ui.status(
                _("Uploaded %s bytes in %0.3f seconds (%0.2f Mbit/s, %0.2f MiB/s)\n")
                % (
                    bytecount,
                    endtime - starttime,
                    8 * byterate / 1000000,
                    byterate / (1024 * 1024),
                ),
                component="debugnetwork",
            )
    return True


@command("debugnetwork", cmdutil.remoteopts, _("[REMOTE]"))
def debugnetwork(ui, repo, remote="default", **opts):
    """debug the network connection to a remote"""

    ui.status(_("Remote name: %s\n") % remote, component="debugnetwork")

    path, branches = hg.parseurl(repo.ui.expandpath(remote))
    ui.status(_("Remote url: %s\n") % path, component="debugnetwork")

    url = util.url(path)
    if url.scheme != "ssh":
        ui.status(_("Not checking network as remote is not an ssh peer"))
        return 1

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

    if not checkhgspeed(ui, url, opts):
        msg = _("Failed to check Mercurial server connection speed.\n")
        ui.status(msg, component=_("debugnetwork"))
        return 6
