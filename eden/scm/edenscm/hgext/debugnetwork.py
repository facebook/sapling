# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""test network connections to the server

::

    [debugnetwork]
    # Set how many bytes to download in the download test.
    speed-test-download-size = 10M

    # Set how many bytes to upload in the upload test.
    speed-test-upload-size = 1M
"""

import socket

from edenscm.mercurial import (
    error,
    hg,
    httpclient,
    httpconnection,
    registrar,
    sslutil,
    util,
)
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

SOCKET_AF = {
    getattr(socket, field): field for field in dir(socket) if field.startswith("AF_")
}
BLOCK_SIZE = 2 * 1024 * 1024
HEADER_MONONOKE_HOST = "x-mononoke-host"
HEADER_NETSPEEDTEST_NBYTES = "x-netspeedtest-nbytes"


def httpstatussuccess(s):
    return s >= 200 and s < 300


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


def checkmononokehost(ui, url, opts):
    sslvalidator = lambda x: None if opts.get("insecure") else sslutil.validatesocket

    _authdata, auth = httpconnection.readauthforuri(ui, str(url), url.user)

    conn = httpclient.HTTPConnection(
        url.host,
        int(url.port),
        use_ssl=True,
        ssl_wrap_socket=sslutil.wrapsocket,
        ssl_validator=sslvalidator,
        ui=ui,
        certfile=auth.get("cert"),
        keyfile=auth.get("key"),
    )

    conn.request(b"GET", b"/health_check", body=None, headers=None)
    res = conn.getresponse()
    while not res.complete():
        res.read(length=BLOCK_SIZE)
    if not httpstatussuccess(res.status):
        raise error.Abort(
            "checkmononokehost: HTTP response status code: %s", res.status
        )

    hostname = res.headers.get(HEADER_MONONOKE_HOST)
    if hostname:
        ui.status(_("Server hostname is %s\n") % hostname, component="debugnetwork")
    return True


def checkspeedhttp(ui, url, opts):
    ui.status(_("Testing connection speed to the server\n"), component="debugnetwork")
    download = ui.configbytes("debugnetwork", "speed-test-download-size", 10000000)
    upload = ui.configbytes("debugnetwork", "speed-test-upload-size", 1000000)
    unixsocketpath = ui.config("auth_proxy", "unix_socket_path")

    if unixsocketpath:
        ui.status(_("Traffic will go through the x2pagentd."), component="debugnetwork")
        conn = httpclient.HTTPConnection(
            url.host,
            use_ssl=False,
            ui=ui,
            unix_socket_path=unixsocketpath,
        )
    else:

        sslvalidator = (
            lambda x: None if opts.get("insecure") else sslutil.validatesocket
        )

        _authdata, auth = httpconnection.readauthforuri(ui, str(url), url.user)

        conn = httpclient.HTTPConnection(
            url.host,
            int(url.port),
            use_ssl=True,
            ssl_wrap_socket=sslutil.wrapsocket,
            ssl_validator=sslvalidator,
            ui=ui,
            certfile=auth.get("cert"),
            keyfile=auth.get("key"),
        )

    def downloadtest(_description, bytecount):
        headers = {HEADER_NETSPEEDTEST_NBYTES: bytecount}
        conn.request(b"GET", b"/netspeedtest", body=None, headers=headers)
        starttime = util.timer()
        res = conn.getresponse()
        while not res.complete():
            res.read(length=BLOCK_SIZE)
        endtime = util.timer()
        if not httpstatussuccess(res.status):
            raise error.Abort("downloadtest: HTTP response status code: %s", res.status)

        return endtime - starttime

    def uploadtest(_description, bytecount):
        body = bytecount * b"A"
        starttime = util.timer()
        conn.request(b"POST", b"/netspeedtest", body=body)
        res = conn.getresponse()
        while not res.complete():
            res.read(length=BLOCK_SIZE)
        endtime = util.timer()

        if not httpstatussuccess(res.status):
            raise error.Abort("uploadtest: HTTP response status code: %s" % res.status)
        return endtime - starttime

    def latencytest(n):
        latencies = []
        while n > 0:
            conn.request(b"GET", b"/health_check", body=None)
            starttime = util.timer()
            res = conn.getresponse()
            while not res.complete():
                res.read(length=BLOCK_SIZE)
            endtime = util.timer()
            if not httpstatussuccess(res.status):
                raise error.Abort(
                    "latencytest: HTTP response status code: %s" % res.status
                )
            latencies.append(endtime - starttime)
            n -= 1
        return latencies

    res = drivespeedtests(
        ui,
        (latencytest, 5),
        (downloadtest, "download", download),
        (uploadtest, "upload", upload),
    )

    conn.close()

    return res


def drivespeedtests(ui, latency, upload, download):
    latencytest, latency_ntests = latency
    uploadtest, testname, bytecount = upload
    downloadtest, testname, bytecount = download

    def printspeedresult(testname, bytecount, testtime):
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
        latencies = latencytest(latency_ntests)
        latency = sum(latencies, 0) / len(latencies)
        ui.status(
            _("Latency: %s (average of %s round-trips)\n")
            % (util.timecount(latency), len(latencies)),
            component="debugnetwork",
        )

        for testfunc, testname, bytecount in [upload, download]:
            warmuptime = testfunc("warming up for %s test" % testname, bytecount)
            if warmuptime < 0.2:
                # The network is sufficiently fast that we warmed up in <200ms.
                # To make the test more meaningful, increase the size of data
                # 25x (which should give a maximum test time of 5s).
                bytecount *= 25
                warmuptime = testfunc(
                    "warming up for large %s test" % testname, bytecount
                )
            printspeedresult("(round 1) %sed" % testname, bytecount, warmuptime)
            testtime = testfunc(testname, bytecount)
            printspeedresult("(round 2) %sed" % testname, bytecount, testtime)
        return True
    except Exception as e:
        ui.warn(_("error testing speed: %s\n") % e)
        return False


@command(
    "debugnetwork",
    [
        ("", "connection", False, _("run connection tests")),
        ("", "speed", False, _("run speed tests")),
    ],
    _("[REMOTE]"),
)
def debugnetwork(ui, repo, remote="default", **opts):
    """debug the network connection to a remote"""

    alltests = not any(opts.get(opt) for opt in ["connection", "speed"])

    ui.status(_("Remote name: %s\n") % remote, component="debugnetwork")

    path, branches = hg.parseurl(repo.ui.expandpath(remote))
    ui.status(_("Remote url: %s\n") % path, component="debugnetwork")

    url = util.url(path)

    if url.port is None:
        url.port = "443"

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

        if not checkmononokehost(ui, url, opts):
            msg = _("Failed to connect to check Mononoke server hostname.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 4

    if alltests or opts.get("speed"):
        if not checkspeedhttp(ui, url, opts):
            msg = _("Failed to check Mononoke server connection speed.\n")
            ui.status(msg, component=_("debugnetwork"))
            return 6
