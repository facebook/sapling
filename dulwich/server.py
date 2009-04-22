# server.py -- Implementation of the server side git protocols
# Copryight (C) 2008 John Carr <john.carr@unrouted.co.uk>
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

import SocketServer
import tempfile

from protocol import (
    Protocol,
    ProtocolFile,
    TCP_GIT_PORT,
    extract_capabilities,
    )
from repo import (
    Repo,
    )
from pack import (
    write_pack_data,
    )

class Backend(object):

    def get_refs(self):
        """
        Get all the refs in the repository

        :return: dict of name -> sha
        """
        raise NotImplementedError

    def apply_pack(self, refs, read):
        """ Import a set of changes into a repository and update the refs

        :param refs: list of tuple(name, sha)
        :param read: callback to read from the incoming pack
        """
        raise NotImplementedError

    def fetch_objects(self, determine_wants, graph_walker, progress):
        """
        Yield the objects required for a list of commits.

        :param progress: is a callback to send progress messages to the client
        """
        raise NotImplementedError


class GitBackend(Backend):

    def __init__(self, gitdir=None):
        self.gitdir = gitdir

        if not self.gitdir:
            self.gitdir = tempfile.mkdtemp()
            Repo.create(self.gitdir)

        self.repo = Repo(self.gitdir)
        self.fetch_objects = self.repo.fetch_objects
        self.get_refs = self.repo.get_refs

    def apply_pack(self, refs, read):
        fd, commit = self.repo.object_store.add_thin_pack()
        fd.write(read())
        fd.close()
        commit()

        for oldsha, sha, ref in refs:
            if ref == "0" * 40:
                self.repo.remove_ref(ref)
            else:
                self.repo.set_ref(ref, sha)

        print "pack applied"


class Handler(object):

    def __init__(self, backend, read, write):
        self.backend = backend
        self.proto = Protocol(read, write)

    def capabilities(self):
        return " ".join(self.default_capabilities())


class UploadPackHandler(Handler):

    def default_capabilities(self):
        return ("multi_ack", "side-band-64k", "thin-pack", "ofs-delta")

    def handle(self):
        def determine_wants(heads):
            keys = heads.keys()
            if keys:
                self.proto.write_pkt_line("%s %s\x00%s\n" % ( heads[keys[0]], keys[0], self.capabilities()))
                for k in keys[1:]:
                    self.proto.write_pkt_line("%s %s\n" % (heads[k], k))

            # i'm done..
            self.proto.write("0000")

            # Now client will either send "0000", meaning that it doesnt want to pull.
            # or it will start sending want want want commands
            want = self.proto.read_pkt_line()
            if want == None:
                return []

            want, self.client_capabilities = extract_capabilities(want)

            want_revs = []
            while want and want[:4] == 'want':
                want_revs.append(want[5:45])
                want = self.proto.read_pkt_line()
            return want_revs

        progress = lambda x: self.proto.write_sideband(2, x)
        write = lambda x: self.proto.write_sideband(1, x)

        class ProtocolGraphWalker(object):

            def __init__(self, proto):
                self.proto = proto
                self._last_sha = None

            def ack(self, have_ref):
                self.proto.write_pkt_line("ACK %s continue\n" % have_ref)

            def next(self):
                have = self.proto.read_pkt_line()
                if have[:4] == 'have':
                    return have[5:45]

                #if have[:4] == 'done':
                #    return None

                if self._last_sha:
                    # Oddness: Git seems to resend the last ACK, without the "continue" statement
                    self.proto.write_pkt_line("ACK %s\n" % self._last_sha)

                # The exchange finishes with a NAK
                self.proto.write_pkt_line("NAK\n")

        graph_walker = ProtocolGraphWalker(self.proto)
        num_objects, objects_iter = self.backend.fetch_objects(determine_wants, graph_walker, progress)

        # Do they want any objects?
        if num_objects == 0:
            return

        progress("dul-daemon says what\n")
        progress("counting objects: %d, done.\n" % num_objects)
        write_pack_data(ProtocolFile(None, write), objects_iter, num_objects)
        progress("how was that, then?\n")
        # we are done
        self.proto.write("0000")


class ReceivePackHandler(Handler):

    def default_capabilities(self):
        return ("report-status", "delete-refs")

    def handle(self):
        refs = self.backend.get_refs().items()

        if refs:
            self.proto.write_pkt_line("%s %s\x00%s\n" % (refs[0][1], refs[0][0], self.capabilities()))
            for i in range(1, len(refs)):
                ref = refs[i]
                self.proto.write_pkt_line("%s %s\n" % (ref[1], ref[0]))
        else:
            self.proto.write_pkt_line("0000000000000000000000000000000000000000 capabilities^{} %s" % self.capabilities())

        self.proto.write("0000")

        client_refs = []
        ref = self.proto.read_pkt_line()

        # if ref is none then client doesnt want to send us anything..
        if ref is None:
            return

        ref, client_capabilities = extract_capabilities(ref)

        # client will now send us a list of (oldsha, newsha, ref)
        while ref:
            client_refs.append(ref.split())
            ref = self.proto.read_pkt_line()

        # backend can now deal with this refs and read a pack using self.read
        self.backend.apply_pack(client_refs, self.proto.read)

        # when we have read all the pack from the client, it assumes everything worked OK
        # there is NO ack from the server before it reports victory.


class TCPGitRequestHandler(SocketServer.StreamRequestHandler):

    def handle(self):
        proto = Protocol(self.rfile.read, self.wfile.write)
        command, args = proto.read_cmd()

        # switch case to handle the specific git command
        if command == 'git-upload-pack':
            cls = UploadPackHandler
        elif command == 'git-receive-pack':
            cls = ReceivePackHandler
        else:
            return

        h = cls(self.server.backend, self.rfile.read, self.wfile.write)
        h.handle()


class TCPGitServer(SocketServer.TCPServer):

    allow_reuse_address = True
    serve = SocketServer.TCPServer.serve_forever

    def __init__(self, backend, listen_addr, port=TCP_GIT_PORT):
        self.backend = backend
        SocketServer.TCPServer.__init__(self, (listen_addr, port), TCPGitRequestHandler)


