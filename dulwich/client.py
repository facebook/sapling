# client.py -- Implementation of the server side git protocols
# Copyright (C) 2008-2009 Jelmer Vernooij <jelmer@samba.org>
# Copyright (C) 2008 John Carr
#
# This program is free software; you can redistribute it and/or
# modify it under the terms of the GNU General Public License
# as published by the Free Software Foundation; either version 2
# or (at your option) a later version of the License.
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

"""Client side support for the Git protocol."""

__docformat__ = 'restructuredText'

import os
import select
import socket
import subprocess

from errors import (
    ChecksumMismatch,
    )
from protocol import (
    Protocol,
    TCP_GIT_PORT,
    extract_capabilities,
    )
from pack import (
    write_pack_data,
    )


def _fileno_can_read(fileno):
    """Check if a file descriptor is readable."""
    return len(select.select([fileno], [], [], 0)[0]) > 0


class SimpleFetchGraphWalker(object):
    """Graph walker that finds out what commits are missing."""

    def __init__(self, local_heads, get_parents):
        """Create a new SimpleFetchGraphWalker instance.

        :param local_heads: SHA1s that should be retrieved
        :param get_parents: Function for finding the parents of a SHA1.
        """
        self.heads = set(local_heads)
        self.get_parents = get_parents
        self.parents = {}

    def ack(self, sha):
        """Ack that a particular revision and its ancestors are present in the target."""
        if sha in self.heads:
            self.heads.remove(sha)
        if sha in self.parents:
            for p in self.parents[sha]:
                self.ack(p)

    def next(self):
        """Iterate over revisions that might be missing in the target."""
        if self.heads:
            ret = self.heads.pop()
            ps = self.get_parents(ret)
            self.parents[ret] = ps
            self.heads.update(ps)
            return ret
        return None


CAPABILITIES = ["multi_ack", "side-band-64k", "ofs-delta"]


class GitClient(object):
    """Git smart server client.

    """

    def __init__(self, can_read, read, write, thin_packs=True, 
        report_activity=None):
        """Create a new GitClient instance.

        :param can_read: Function that returns True if there is data available
            to be read.
        :param read: Callback for reading data, takes number of bytes to read
        :param write: Callback for writing data
        :param thin_packs: Whether or not thin packs should be retrieved
        :param report_activity: Optional callback for reporting transport
            activity.
        """
        self.proto = Protocol(read, write, report_activity)
        self._can_read = can_read
        self._capabilities = list(CAPABILITIES)
        #if thin_packs:
        #    self._capabilities.append("thin-pack")

    def capabilities(self):
        return " ".join(self._capabilities)

    def read_refs(self):
        server_capabilities = None
        refs = {}
        # Receive refs from server
        for pkt in self.proto.read_pkt_seq():
            (sha, ref) = pkt.rstrip("\n").split(" ", 1)
            if server_capabilities is None:
                (ref, server_capabilities) = extract_capabilities(ref)
            refs[ref] = sha
        return refs, server_capabilities

    def send_pack(self, path, determine_wants, generate_pack_contents):
        """Upload a pack to a remote repository.

        :param path: Repository path
        :param generate_pack_contents: Function that can return the shas of the 
            objects to upload.
        """
        refs, server_capabilities = self.read_refs()
        changed_refs = determine_wants(refs)
        if not changed_refs:
            self.proto.write_pkt_line(None)
            return {}
        want = []
        have = []
        sent_capabilities = False
        for changed_ref, new_sha1 in changed_refs.iteritems():
            old_sha1 = refs.get(changed_ref, "0" * 40)
            if sent_capabilities:
                self.proto.write_pkt_line("%s %s %s" % (old_sha1, new_sha1, changed_ref))
            else:
                self.proto.write_pkt_line("%s %s %s\0%s" % (old_sha1, new_sha1, changed_ref, self.capabilities()))
                sent_capabilities = True
            want.append(new_sha1)
            if old_sha1 != "0"*40:
                have.append(old_sha1)
        self.proto.write_pkt_line(None)
        objects = generate_pack_contents(want, have)
        (entries, sha) = write_pack_data(self.proto.write_file(), objects, len(objects))
        self.proto.write(sha)
        
        # read the final confirmation sha
        client_sha = self.proto.read(20)
        if not client_sha in (None, sha):
            raise ChecksumMismatch(sha, client_sha)
            
        return changed_refs

    def fetch_pack(self, path, determine_wants, graph_walker, pack_data,
                   progress):
        """Retrieve a pack from a git smart server.

        :param determine_wants: Callback that returns list of commits to fetch
        :param graph_walker: Object with next() and ack().
        :param pack_data: Callback called for each bit of data in the pack
        :param progress: Callback for progress reports (strings)
        """
        (refs, server_capabilities) = self.read_refs()
        wants = determine_wants(refs)
        if not wants:
            self.proto.write_pkt_line(None)
            return refs
        self.proto.write_pkt_line("want %s %s\n" % (wants[0], self.capabilities()))
        for want in wants[1:]:
            self.proto.write_pkt_line("want %s\n" % want)
        self.proto.write_pkt_line(None)
        have = graph_walker.next()
        while have:
            self.proto.write_pkt_line("have %s\n" % have)
            if self._can_read():
                pkt = self.proto.read_pkt_line()
                parts = pkt.rstrip("\n").split(" ")
                if parts[0] == "ACK":
                    graph_walker.ack(parts[1])
                    assert parts[2] == "continue"
            have = graph_walker.next()
        self.proto.write_pkt_line("done\n")
        pkt = self.proto.read_pkt_line()
        while pkt:
            parts = pkt.rstrip("\n").split(" ")
            if parts[0] == "ACK":
                graph_walker.ack(pkt.split(" ")[1])
            if len(parts) < 3 or parts[2] != "continue":
                break
            pkt = self.proto.read_pkt_line()
        for pkt in self.proto.read_pkt_seq():
            channel = ord(pkt[0])
            pkt = pkt[1:]
            if channel == 1:
                pack_data(pkt)
            elif channel == 2:
                progress(pkt)
            else:
                raise AssertionError("Invalid sideband channel %d" % channel)
        return refs


class TCPGitClient(GitClient):
    """A Git Client that works over TCP directly (i.e. git://)."""

    def __init__(self, host, port=None, *args, **kwargs):
        self._socket = socket.socket(type=socket.SOCK_STREAM)
        if port is None:
            port = TCP_GIT_PORT
        self._socket.connect((host, port))
        self.rfile = self._socket.makefile('rb', -1)
        self.wfile = self._socket.makefile('wb', 0)
        self.host = host
        super(TCPGitClient, self).__init__(lambda: _fileno_can_read(self._socket.fileno()), self.rfile.read, self.wfile.write, *args, **kwargs)

    def send_pack(self, path, changed_refs, generate_pack_contents):
        """Send a pack to a remote host.

        :param path: Path of the repository on the remote host
        """
        self.proto.send_cmd("git-receive-pack", path, "host=%s" % self.host)
        return super(TCPGitClient, self).send_pack(path, changed_refs, generate_pack_contents)

    def fetch_pack(self, path, determine_wants, graph_walker, pack_data, progress):
        """Fetch a pack from the remote host.
        
        :param path: Path of the reposiutory on the remote host
        :param determine_wants: Callback that receives available refs dict and 
            should return list of sha's to fetch.
        :param graph_walker: GraphWalker instance used to find missing shas
        :param pack_data: Callback for writing pack data
        :param progress: Callback for writing progress
        """
        self.proto.send_cmd("git-upload-pack", path, "host=%s" % self.host)
        return super(TCPGitClient, self).fetch_pack(path, determine_wants,
            graph_walker, pack_data, progress)


class SubprocessGitClient(GitClient):
    """Git client that talks to a server using a subprocess."""

    def __init__(self, *args, **kwargs):
        self.proc = None
        self._args = args
        self._kwargs = kwargs

    def _connect(self, service, *args, **kwargs):
        argv = [service] + list(args)
        self.proc = subprocess.Popen(argv, bufsize=0,
                                stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE)
        def read_fn(size):
            return self.proc.stdout.read(size)
        def write_fn(data):
            self.proc.stdin.write(data)
            self.proc.stdin.flush()
        return GitClient(lambda: _fileno_can_read(self.proc.stdout.fileno()), read_fn, write_fn, *args, **kwargs)

    def send_pack(self, path, changed_refs, generate_pack_contents):
        """Upload a pack to the server.

        :param path: Path to the git repository on the server
        :param changed_refs: Dictionary with new values for the refs
        :param generate_pack_contents: Function that returns an iterator over 
            objects to send
        """
        client = self._connect("git-receive-pack", path)
        return client.send_pack(path, changed_refs, generate_pack_contents)

    def fetch_pack(self, path, determine_wants, graph_walker, pack_data, 
        progress):
        """Retrieve a pack from the server

        :param path: Path to the git repository on the server
        :param determine_wants: Function that receives existing refs 
            on the server and returns a list of desired shas
        :param graph_walker: GraphWalker instance
        :param pack_data: Function that can write pack data
        :param progress: Function that can write progress texts
        """
        client = self._connect("git-upload-pack", path)
        return client.fetch_pack(path, determine_wants, graph_walker, pack_data,
                                 progress)


class SSHSubprocess(object):
    """A socket-like object that talks to an ssh subprocess via pipes."""

    def __init__(self, proc):
        self.proc = proc

    def send(self, data):
        return os.write(self.proc.stdin.fileno(), data)

    def recv(self, count):
        return self.proc.stdout.read(count)

    def close(self):
        self.proc.stdin.close()
        self.proc.stdout.close()
        self.proc.wait()


class SSHVendor(object):

    def connect_ssh(self, host, command, username=None, port=None):
        #FIXME: This has no way to deal with passwords..
        args = ['ssh', '-x']
        if port is not None:
            args.extend(['-p', str(port)])
        if username is not None:
            host = "%s@%s" % (username, host)
        args.append(host)
        proc = subprocess.Popen(args + command,
                                stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE)
        return SSHSubprocess(proc)

# Can be overridden by users
get_ssh_vendor = SSHVendor


class SSHGitClient(GitClient):

    def __init__(self, host, port=None, *args, **kwargs):
        self.host = host
        self.port = port
        self._args = args
        self._kwargs = kwargs

    def send_pack(self, path, determine_wants, generate_pack_contents):
        remote = get_ssh_vendor().connect_ssh(self.host, ["git-receive-pack %s" % path], port=self.port)
        client = GitClient(lambda: _fileno_can_read(remote.proc.stdout.fileno()), remote.recv, remote.send, *self._args, **self._kwargs)
        return client.send_pack(path, determine_wants, generate_pack_contents)

    def fetch_pack(self, path, determine_wants, graph_walker, pack_data,
        progress):
        remote = get_ssh_vendor().connect_ssh(self.host, ["git-upload-pack %s" % path], port=self.port)
        client = GitClient(lambda: _fileno_can_read(remote.proc.stdout.fileno()), remote.recv, remote.send, *self._args, **self._kwargs)
        return client.fetch_pack(path, determine_wants, graph_walker, pack_data,
                                 progress)

