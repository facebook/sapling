#!/usr/bin/env python

"""dummy SMTP server for use in tests"""

from __future__ import absolute_import

import asyncore
import optparse
import smtpd
import ssl
import sys

from edenscm.mercurial import server, sslutil, ui as uimod


def log(msg):
    sys.stdout.write(msg)
    sys.stdout.flush()


class dummysmtpserver(smtpd.SMTPServer):
    def __init__(self, localaddr):
        smtpd.SMTPServer.__init__(self, localaddr, remoteaddr=None)

    def process_message(self, peer, mailfrom, rcpttos, data):
        log("%s from=%s to=%s\n" % (peer[0], mailfrom, ", ".join(rcpttos)))


class dummysmtpsecureserver(dummysmtpserver):
    def __init__(self, localaddr, certfile):
        dummysmtpserver.__init__(self, localaddr)
        self._certfile = certfile

    def handle_accept(self):
        pair = self.accept()
        if not pair:
            return
        conn, addr = pair
        ui = uimod.ui.load()
        try:
            # wrap_socket() would block, but we don't care
            conn = sslutil.wrapserversocket(conn, ui, certfile=self._certfile)
        except ssl.SSLError:
            log("%s ssl error\n" % addr[0])
            conn.close()
            return
        smtpd.SMTPChannel(self, conn, addr)


def run():
    try:
        asyncore.loop()
    except KeyboardInterrupt:
        pass


def main():
    op = optparse.OptionParser()
    op.add_option("-d", "--daemon", action="store_true")
    op.add_option("--daemon-postexec", action="append")
    op.add_option("-p", "--port", type=int, default=8025)
    op.add_option("-a", "--address", default="localhost")
    op.add_option("--pid-file", metavar="FILE")
    op.add_option("--tls", choices=["none", "smtps"], default="none")
    op.add_option("--certificate", metavar="FILE")

    opts, args = op.parse_args()
    if opts.tls == "smtps" and not opts.certificate:
        op.error("--certificate must be specified")

    addr = (opts.address, opts.port)

    def init():
        if opts.tls == "none":
            dummysmtpserver(addr)
        else:
            dummysmtpsecureserver(addr, opts.certificate)
        log("listening at %s:%d\n" % addr)

    server.runservice(
        vars(opts),
        initfn=init,
        runfn=run,
        runargs=[sys.executable, __file__] + sys.argv[1:],
    )


if __name__ == "__main__":
    main()
