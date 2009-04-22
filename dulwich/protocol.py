# protocol.py -- Shared parts of the git protocols
# Copryight (C) 2008 John Carr <john.carr@unrouted.co.uk>
# Copyright (C) 2008 Jelmer Vernooij <jelmer@samba.org>
#
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; version 2
# or (at your option) any later version of the License.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston,
# MA  02110-1301, USA.

"""Generic functions for talking the git smart server protocol."""

import socket

from errors import (
    HangupException,
    GitProtocolError,
    )

TCP_GIT_PORT = 9418

class ProtocolFile(object):
    """
    Some network ops are like file ops. The file ops expect to operate on
    file objects, so provide them with a dummy file.
    """

    def __init__(self, read, write):
        self.read = read
        self.write = write

    def tell(self):
        pass

    def close(self):
        pass


class Protocol(object):

    def __init__(self, read, write, report_activity=None):
        self.read = read
        self.write = write
        self.report_activity = report_activity

    def read_pkt_line(self):
        """
        Reads a 'pkt line' from the remote git process

        :return: The next string from the stream
        """
        try:
            sizestr = self.read(4)
            if not sizestr:
                raise HangupException()
            size = int(sizestr, 16)
            if size == 0:
                if self.report_activity:
                    self.report_activity(4, 'read')
                return None
            if self.report_activity:
                self.report_activity(size, 'read')
            return self.read(size-4)
        except socket.error, e:
            raise GitProtocolError(e)

    def read_pkt_seq(self):
        pkt = self.read_pkt_line()
        while pkt:
            yield pkt
            pkt = self.read_pkt_line()

    def write_pkt_line(self, line):
        """
        Sends a 'pkt line' to the remote git process

        :param line: A string containing the data to send
        """
        try:
            if line is None:
                self.write("0000")
                if self.report_activity:
                    self.report_activity(4, 'write')
            else:
                self.write("%04x%s" % (len(line)+4, line))
                if self.report_activity:
                    self.report_activity(4+len(line), 'write')
        except socket.error, e:
            raise GitProtocolError(e)

    def write_sideband(self, channel, blob):
        """
        Write data to the sideband (a git multiplexing method)

        :param channel: int specifying which channel to write to
        :param blob: a blob of data (as a string) to send on this channel
        """
        # a pktline can be a max of 65520. a sideband line can therefore be
        # 65520-5 = 65515
        # WTF: Why have the len in ASCII, but the channel in binary.
        while blob:
            self.write_pkt_line("%s%s" % (chr(channel), blob[:65515]))
            blob = blob[65515:]

    def send_cmd(self, cmd, *args):
        """
        Send a command and some arguments to a git server

        Only used for git://

        :param cmd: The remote service to access
        :param args: List of arguments to send to remove service
        """
        self.write_pkt_line("%s %s" % (cmd, "".join(["%s\0" % a for a in args])))

    def read_cmd(self):
        """
        Read a command and some arguments from the git client

        Only used for git://

        :return: A tuple of (command, [list of arguments])
        """
        line = self.read_pkt_line()
        splice_at = line.find(" ")
        cmd, args = line[:splice_at], line[splice_at+1:]
        assert args[-1] == "\x00"
        return cmd, args[:-1].split(chr(0))


def extract_capabilities(text):
    """Extract a capabilities list from a string, if present.

    :param text: String to extract from
    :return: Tuple with text with capabilities removed and list of 
        capabilities or None (if no capabilities were present.
    """
    if not "\0" in text:
        return text, None
    capabilities = text.split("\0")
    return (capabilities[0], capabilities[1:])

