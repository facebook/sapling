# server.py - inotify common protocol code
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import cStringIO, socket, struct

"""
  Protocol between inotify clients and server:

  Client sending query:
  1) send protocol version number
  2) send query type (string, 4 letters long)
  3) send query parameters:
     - For STAT, N+1 \0-separated strings:
        1) N different names that need checking
        2) 1 string containing all the status types to match
     - No parameter needed for DBUG

  Server sending query answer:
  1) send protocol version number
  2) send query type
  3) send struct.pack'ed headers describing the length of the content:
      e.g. for STAT, receive 8 integers describing the length of the
      8 \0-separated string lists ( one list for each lmar!?ic status type )

"""

version = 2

resphdrfmts = {
    'STAT': '>llllllll', # status requests
    'DBUG': '>l'         # debugging queries
}
resphdrsizes = dict((k, struct.calcsize(v))
                    for k, v in resphdrfmts.iteritems())

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
