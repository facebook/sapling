# server.py - inotify common protocol code
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import cStringIO, socket, struct

version = 1

resphdrfmt = '>llllllll'
resphdrsize = struct.calcsize(resphdrfmt)

def recvcs(sock):
    cs = cStringIO.StringIO()
    s = True
    try:
        while s:
            s = sock.recv(65536)
            cs.write(s)
    finally:
        sock.shutdown(socket.SHUT_RD)
    cs.seek(0)
    return cs
