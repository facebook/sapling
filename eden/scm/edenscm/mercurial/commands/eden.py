# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# no-check-code

import argparse
import collections
import logging
import os
import struct
import sys
import time
import typing
from typing import IO, Any, Callable, Optional, Tuple, TypeVar

from .. import (
    context,
    error,
    extensions,
    hg,
    localrepo,
    pycompat,
    scmutil,
    txnutil,
    ui,
    util,
)
from ..i18n import _
from ..node import bin, hex
from .cmdtable import command


if pycompat.iswindows:
    from msvcrt import open_osfhandle

    def fdopen(handle, mode):
        # type: (int, str) -> IO[Any]
        os_mode = os.O_WRONLY if mode == "wb" else os.O_RDONLY
        fileno = open_osfhandle(handle, os_mode)
        return util.fdopen(fileno, mode)


else:

    def fdopen(handle, mode):
        # type: (int, str) -> IO[Any]
        return util.fdopen(handle, mode)


AttributeType = TypeVar("AttributeType")


#
# Message chunk header format.
# (This is a format argument for struct.unpack())
#
# The header consists of 4 big-endian 32-bit unsigned integers:
#
# - Transaction ID
#   This is a numeric identifier used for associating a response with a given
#   request.  The response for a particular request will always contain the
#   same transaction ID as was sent in the request.  (Currently responses are
#   always sent in the same order that requests were received, so this is
#   primarily used just as a sanity check.)
#
# - Command ID
#   This is one of the CMD_* constants below.
#
# - Flags
#   This is a bit set of the FLAG_* constants defined below.
#
# - Data length
#   This lists the number of body bytes sent with this request/response.
#   The body is sent immediately after the header data.
#
HEADER_FORMAT = b">IIII"
HEADER_SIZE = 16

# The length of a SHA-1 hash
SHA1_NUM_BYTES = 20

# The protocol version number.
#
# Increment this any time you add new commands or make changes to the data
# format sent between edenfs and the hg_import_helper.
#
# In general we do not need to worry about backwards/forwards compatibility of
# the protocol, since edenfs and the hg_import_helper.py script should always
# be updated together.  This protocol version ID allows us to sanity check that
# edenfs is actually talking to the correct hg_import_helper.py script,
# and to fail if it somehow is using an import helper script from the wrong
# release.
#
# This must be kept in sync with the PROTOCOL_VERSION field in the C++
# HgImporter code.
PROTOCOL_VERSION = 1

START_FLAGS_TREEMANIFEST_SUPPORTED = 0x01
START_FLAGS_MONONOKE_SUPPORTED = 0x02
START_FLAGS_CAT_TREE_SUPPORTED = 0x04

#
# Message types.
#
# See the specific cmd_* functions below for documentation on the
# request/response formats.
#
CMD_STARTED = 0
CMD_RESPONSE = 1
CMD_MANIFEST = 2
CMD_OLD_CAT_FILE = 3
CMD_MANIFEST_NODE_FOR_COMMIT = 4
CMD_FETCH_TREE = 5
CMD_PREFETCH_FILES = 6
CMD_CAT_FILE = 7
CMD_GET_FILE_SIZE = 8
CMD_CAT_TREE = 9

#
# Flag values.
#
# The flag values are intended to be bitwise-ORed with each other.
#

# FLAG_ERROR:
# - This flag is only valid in response chunks.  This indicates that an error
#   has occurred.  The chunk body contains the error message.  Any chunks
#   received prior to the error chunk should be ignored.
FLAG_ERROR = 0x01
# FLAG_MORE_CHUNKS:
# - If this flag is set, there are more chunks to come that are part of the
#   same request/response.  If this flag is not set, this is the final chunk in
#   this request/response.
FLAG_MORE_CHUNKS = 0x02


class Request(object):
    def __init__(self, txn_id, command, flags, body):
        # type: (int, int, int, bytes) -> None
        self.txn_id = txn_id
        self.command = command
        self.flags = flags
        self.body = body


class ResetRepoError(Exception):
    """This exception type indicates that the internal mercurial repo object state
    is likely bad, and should be re-opened.  We throw this error up to the C++ code,
    and it will restart us when this happens.

    Completely restarting the python hg_import_helper.py script in this case appears to
    be much better than trying to close and re-open the repository in the same python
    process.  The python code tends to leak memory and other resources (e.g.,
    remotefilelog.cacheprocess subprocesses) if we do not restart the process
    completely.
    """

    def __init__(self, original_error):
        # type: (Exception) -> None
        super(ResetRepoError, self).__init__(
            "hg repository needs to be re-opened: %s" % (original_error,)
        )


def cmd(command_id):
    # type: (int) -> Callable[[Callable[[Request], None]], Callable[[Request], None]]
    """
    A helper function for identifying command functions
    """

    def decorator(func):
        # type: (Callable[[Request], None]) -> Callable[[Request], None]
        # pyre-fixme[16]: Anonymous callable has no attribute `__COMMAND_ID__`.
        func.__COMMAND_ID__ = command_id
        return func

    return decorator


class HgUI(ui.ui):
    def __init__(self, src=None):
        # type: (Optional[ui.ui]) -> None
        super(HgUI, self).__init__(src=src)
        # Always print to stderr, never to stdout.
        # We normally use stdout as the pipe to communicate with the main
        # edenfs daemon, and if mercurial prints messages to stdout it can
        # interfere with this communication.
        # This also matches the logging behavior of the main edenfs process,
        # which always logs to stderr.
        self.fout = sys.stderr
        self.ferr = sys.stderr

    def plain(self, feature=None):
        # type: (Optional[str]) -> bool
        return True

    def interactive(self):
        # type: () -> bool
        return False


class HgServer(object):
    def __init__(self, repo, in_fd=None, out_fd=None):
        # type: (localrepo.localrepository, Optional[int], Optional[int]) -> None
        """
        Create an HgServer.

        repo:
          The mercurial repository object.
        in_fd:
          A file descriptor to use for receiving requests.
          If in_fd is None, stdin will be used.
        out_fd:
          A file descriptor to use for sending responses.
          If in_fd is None, stdout will be used.
        """
        if not in_fd:
            self.in_file = pycompat.stdin
        else:
            self.in_file = fdopen(in_fd, "rb")
        if not out_fd:
            self.out_file = pycompat.stdout
        else:
            self.out_file = fdopen(out_fd, "wb")

        self.repo = repo

        try:
            self.treemanifest = extensions.find("treemanifest")
        except KeyError:
            # The treemanifest extension is not present
            self.treemanifest = None

        # Populate our command dictionary
        self._commands = {}
        for member_name in dir(self):
            value = getattr(self, member_name)
            if not util.safehasattr(value, "__COMMAND_ID__"):
                continue
            self._commands[value.__COMMAND_ID__] = value

        # Fetch the configuration for the depth to use when fetching trees.
        # Setting this to a value larger than 1 causes the CMD_FETCH_TREE code to
        # pre-fetch more children trees in addition to the specific tree that was
        # requested.  This helps avoiding multiple round-trips when traversing into a
        # directory.
        self._treefetchdepth = self.repo.ui.configint("edenfs", "tree-fetch-depth")

    def serve(self):
        # type: () -> int
        # Send a CMD_STARTED response to indicate we have started,
        # and include some information about the repository configuration.
        options_chunk = self._gen_options()
        self._send_chunk(
            txn_id=0, command=CMD_STARTED, flags=0, data_blocks=(options_chunk,)
        )

        while self.process_request():
            pass

        logging.debug("hg_import_helper shutting down normally")
        return 0

    def _is_mononoke_supported(self, name):
        # type: (str) -> bool
        # `name` comes from `repo.name`.
        # The treemanifest extension sets `repo.name` to `remotefilelog.reponame`,
        # falling back to "unknown" if it isn't set.
        # If treemanifest isn't loaded somehow, then `repo.name` will be None.
        # If name is set and not unknown, then we know that we have mononoke
        # available to us.
        return (name is not None) and (name != "unknown")

    def _gen_options(self):
        # type: () -> bytes
        from edenscm.hgext.remotefilelog import shallowutil, constants

        repo_name = getattr(self.repo, "name", None)
        use_treemanifest = (self.treemanifest is not None) and bool(repo_name)
        use_mononoke = use_treemanifest and self._is_mononoke_supported(repo_name)

        flags = 0
        treemanifest_paths = []
        if use_treemanifest:
            flags |= START_FLAGS_TREEMANIFEST_SUPPORTED
            treemanifest_paths = [
                shallowutil.getlocalpackpath(
                    self.repo.svfs.join(""), constants.TREEPACK_CATEGORY
                ),
                shallowutil.getcachepackpath(self.repo, constants.TREEPACK_CATEGORY),
                shallowutil.getlocalpackpath(
                    self.repo.svfs.join(""), constants.FILEPACK_CATEGORY
                ),
                shallowutil.getcachepackpath(self.repo, constants.FILEPACK_CATEGORY),
            ]

        if use_mononoke:
            flags |= START_FLAGS_MONONOKE_SUPPORTED

        flags |= START_FLAGS_CAT_TREE_SUPPORTED

        # Options format:
        # - Protocol version number
        # - Is treemanifest supported?
        # - Number of treemanifest paths
        #   - treemanifest paths, encoded as (length, string_data)
        parts = []
        parts.append(
            struct.pack(">III", PROTOCOL_VERSION, flags, len(treemanifest_paths))
        )
        for path in treemanifest_paths:
            parts.append(struct.pack(">I", len(path)))
            parts.append(pycompat.encodeutf8(path))

        if use_mononoke:
            parts.append(struct.pack(">I", len(repo_name)))
            parts.append(pycompat.encodeutf8(repo_name))

        return b"".join(parts)

    def debug(self, msg, *args, **kwargs):
        # type: (str, Any, Any) -> None
        logging.debug(msg, *args, **kwargs)

    def process_request(self):
        # type: () -> bool
        """
        Process a single request.
        Returns True if the server should continue running, and False if the server
        should exit.
        """
        # Read the request header
        header_data = self.in_file.read(HEADER_SIZE)
        if not header_data:
            # EOF.  All done serving
            return False

        if len(header_data) < HEADER_SIZE:
            raise RuntimeError("received EOF after partial request header")

        header_fields = struct.unpack(HEADER_FORMAT, header_data)
        txn_id, command, flags, data_len = header_fields

        # Read the request body
        body = self.in_file.read(data_len)
        if len(body) < data_len:
            raise RuntimeError("received EOF after partial request")
        req = Request(txn_id, command, flags, body)

        cmd_function = self._commands.get(command)
        if cmd_function is None:
            logging.warning("unknown command %r", command)
            self.send_error(req, "CommandError", "unknown command %r" % (command,))
            return True

        try:
            cmd_function(req)
        except Exception as ex:
            logging.exception("error processing command %r", command)
            self.send_exception(req, ex)

        # Return True to indicate that we should continue serving
        return True

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_MANIFEST)
    def cmd_manifest(self, request):
        # type: (Request) -> None
        """
        Handler for CMD_MANIFEST requests.

        This request asks for the full mercurial manifest contents for a given
        revision.  The response body will be split across one or more chunks.
        (FLAG_MORE_CHUNKS will be set on all but the last chunk.)

        Request body format:
        - Revision name (string)
          This is the mercurial revision ID.  This can be any string that will
          be understood by mercurial to identify a single revision.  (For
          instance, this might be ".", ".^", a 40-character hexadecmial hash,
          or a unique hash prefix, etc.)

        Response body format:
          The response body is a list of manifest entries.  Each manifest entry
          consists of:
          - <rev_hash><tab><flag><tab><path><nul>

          Entry fields:
          - <rev_hash>: The file revision hash, as a 20-byte binary value.
          - <tab>: A literal tab character ('\t')
          - <flag>: The mercurial flag character.  If the mercurial flag is
                    empty this will be omitted.  Valid mercurial flags are:
                    'x': an executable file
                    'l': an symlink
                    '':  a regular file
          - <path>: The full file path, relative to the root of the repository
          - <nul>: a nul byte ('\0')
        """
        rev_name = pycompat.decodeutf8(request.body)
        self.debug("sending manifest for revision %r", rev_name)
        self.dump_manifest(rev_name, request)

    def get_tree(self, path, manifest_node):
        # type: (str, bytes) -> bytes

        # Even though get_tree would fetch the tree if missing, it has a couple
        # of drawbacks. First, for the root manifest, it may prefetch the
        # entire tree, second, it doesn't honor the self._treefetchdepth which
        # avoids a lot of round trips to the server.
        missing = self.repo.manifestlog.datastore.getmissing([(path, manifest_node)])
        if missing:
            try:
                self._fetch_tree_impl(path, manifest_node)
            except Exception as ex:
                logging.warning(
                    "Fetching failed, continuing as this may be spurious: %s", ex
                )

        try:
            return self.repo.manifestlog.datastore.get("", manifest_node)
        except Exception as ex:
            # Now we can raise.
            raise ResetRepoError(ex)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_CAT_TREE)
    def cmd_cat_tree(self, request):
        # type: (Request) -> None
        """
        Handler for CMD_CAT_TREE requests.

        This request asks for the full mercurial manifest contents for a given
        manifest node and path.

        Request body format:
        - Manifest node (binary)
          This is the 20 bytes mercurial node.
        - Path (str)
          This is the path to the directory

        Response body format:
          The response body is a list of manifest entries separated by a new
          line ('\n'). Each manifest entry consists of:
          - <file name><nul><node><flag>

          Entry fields:
          - <file name>: name of the file/directory
          - <nul>: a nul byte ('\0')
          - <node>: the hex file node for the entry. For a directory, this is the manifest node.
          - <flag>: The mercurial flag character. If the mercurial flag is
                    empty, this will be omitted. Valid flags are:
                    'x': an executable file
                    'l': a symlink
                    't': a directory
                    '': a regular file
        """
        if len(request.body) < SHA1_NUM_BYTES:
            raise RuntimeError(
                "cat_tree request data too short: len=%d" % len(request.body)
            )

        if self.treemanifest is None:
            raise RuntimeError("treemanifest not enabled in this repository")

        manifest_node = request.body[:SHA1_NUM_BYTES]
        path = pycompat.decodeutf8(request.body[SHA1_NUM_BYTES:])
        logging.warning(
            "fetching tree for path %r manifest node %s", path, hex(manifest_node)
        )

        tree = self.get_tree(path, manifest_node)

        length_tree = struct.pack(">Q", len(tree))
        self.send_chunk(request, tree, length_tree)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_OLD_CAT_FILE)
    def cmd_old_cat_file(self, request):
        # type: (Request) -> None
        """Handler for CMD_OLD_CAT_FILE requests.

        This requests the contents for a given file.
        New edenfs servers do not send this request, but we still need to support this
        for old edenfs servers that have not been restarted.

        This is similar to CMD_CAT_FILE, but the response body contains only the raw
        file contents.
        """
        if len(request.body) < SHA1_NUM_BYTES + 1:
            raise RuntimeError("old_cat_file request data too short")

        rev_hash = request.body[:SHA1_NUM_BYTES]
        path = pycompat.decodeutf8(request.body[SHA1_NUM_BYTES:])
        self.debug(
            "(pid:%s) CMD_OLD_CAT_FILE request for contents of file %r revision %s",
            os.getpid(),
            path,
            hex(rev_hash),
        )

        contents = self.get_file(path, rev_hash)
        self.send_chunk(request, contents)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_CAT_FILE)
    def cmd_cat_file(self, request):
        # type: (Request) -> None
        """CMD_CAT_FILE: get the contents of a file.

        Request body format:
        - <rev_hash><path>
          Fields:
          - <rev_hash>: The file revision hash, as a 20-byte binary value.
          - <path>: The file path, relative to the root of the repository.

        Response body format:
        - <file_contents>
        - <file_size>
        """
        if len(request.body) < SHA1_NUM_BYTES + 1:
            raise RuntimeError("cat_file request data too short")

        rev_hash = request.body[:SHA1_NUM_BYTES]
        path = pycompat.decodeutf8(request.body[SHA1_NUM_BYTES:])
        self.debug(
            "(pid:%s) getting contents of file %r revision %s",
            os.getpid(),
            path,
            hex(rev_hash),
        )

        contents = self.get_file(path, rev_hash)
        length_data = struct.pack(">Q", len(contents))
        self.send_chunk(request, contents, length_data)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_GET_FILE_SIZE)
    def cmd_get_file_size(self, request):
        # type: (Request) -> None
        """CMD_GET_FILE_SIZE: get the size of a file.

        Request body format:
        - <id><path>
          Fields:
          - <id>: The file revision hash, as a 20-byte binary value
          - <path>: The file path, relative to the root of the repository

        Response body format:
        - <file_size>
        """
        if len(request.body) < SHA1_NUM_BYTES + 1:
            raise RuntimeError("get_file_size request data too short")

        id = request.body[:SHA1_NUM_BYTES]
        path = pycompat.decodeutf8(request.body[SHA1_NUM_BYTES:])

        self.debug("(pid:%s) GET_FILE_SIZE, path %r, id %s", os.getpid(), path, hex(id))
        size = self.get_file_size(path, id)

        data = struct.pack(">Q", size)
        self.send_chunk(request, data)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_MANIFEST_NODE_FOR_COMMIT)
    def cmd_manifest_node_for_commit(self, request):
        # type: (Request) -> None
        """
        Handler for CMD_MANIFEST_NODE_FOR_COMMIT requests.

        Given a commit hash, resolve the manifest node.

        Request body format:
        - Revision name (string)
          This is the mercurial revision ID.  This can be any string that will
          be understood by mercurial to identify a single revision.  (For
          instance, this might be ".", ".^", a 40-character hexadecmial hash,
          or a unique hash prefix, etc.)

        Response body format:
          The response body is the manifest node, a 20-byte binary value.
        """
        rev_name = pycompat.decodeutf8(request.body)
        self.debug("resolving manifest node for revision %r", rev_name)
        try:
            node = self.get_manifest_node(rev_name)
        except error.RepoError as ex:
            # Handle lookup errors explicitly, just so we avoid printing
            # a backtrace in the log if we let this bubble all the way up
            # to the unexpected exception handling code in process_request()
            self.send_exception(request, ex)
            return

        self.send_chunk(request, node)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_FETCH_TREE)
    def cmd_fetch_tree(self, request):
        # type: (Request) -> None
        if len(request.body) < SHA1_NUM_BYTES:
            raise RuntimeError(
                "fetch_tree request data too short: len=%d" % len(request.body)
            )

        manifest_node = request.body[:SHA1_NUM_BYTES]
        path = pycompat.decodeutf8(request.body[SHA1_NUM_BYTES:])
        self.debug(
            "fetching tree for path %r manifest node %s", path, hex(manifest_node)
        )

        self.fetch_tree(path, manifest_node)
        self.send_chunk(request, b"")

    def fetch_tree(self, path, manifest_node):
        # type: (str, bytes) -> None
        if self.treemanifest is None:
            raise RuntimeError("treemanifest not enabled in this repository")

        try:
            self._fetch_tree_impl(path, manifest_node)
            self.repo.commitpending()
        except Exception as ex:
            # Ugh.  Mercurial sometimes throws spurious KeyErrors
            # if this tree was created since we first initialized our
            # connection to the server.
            #
            # These errors come from the server-side; there doesn't seem to be
            # a good way to force the server to re-read the data other than
            # recreating our repo object.
            raise ResetRepoError(ex)

    def _fetch_tree_impl(self, path, manifest_node):
        # type: (str, bytes) -> None
        mfnodes = set([manifest_node])
        if path:
            # We have to call repo._prefetchtrees() directly if we have a path.
            # We cannot compute the set of base nodes in this case.
            self.repo._prefetchtrees(path, mfnodes, [], [], depth=self._treefetchdepth)
        else:
            # When querying the top-level node use repo.prefetchtrees()
            # It will compute a reasonable set of base nodes to send in the query.
            self.repo.prefetchtrees(mfnodes, depth=self._treefetchdepth)

    def send_chunk(self, request, *data, **kwargs):
        # type: (Request, bytes, Any) -> None
        is_last = kwargs.pop("is_last", True)
        if kwargs:
            raise TypeError("unexpected keyword arguments: %r" % (kwargs.keys(),))

        flags = 0
        if not is_last:
            flags |= FLAG_MORE_CHUNKS

        self._send_chunk(
            request.txn_id,
            command=CMD_RESPONSE,
            flags=flags,
            data_blocks=typing.cast(Tuple[bytes], data),
        )

    def send_exception(self, request, exc):
        # type: (Request, Exception) -> None
        self.send_error(request, type(exc).__name__, str(exc))

    def send_error(self, request, error_type, message):
        # type: (Request, str, str) -> None
        txn_id = 0
        if request is not None:
            txn_id = request.txn_id

        data = b"".join(
            [
                struct.pack(b">I", len(error_type)),
                error_type.encode("utf-8", errors="surrogateescape"),
                struct.pack(b">I", len(message)),
                message.encode("utf-8", errors="surrogateescape"),
            ]
        )
        self._send_chunk(
            txn_id, command=CMD_RESPONSE, flags=FLAG_ERROR, data_blocks=(data,)
        )

    def _send_chunk(self, txn_id, command, flags, data_blocks):
        # type: (int, int, int, Tuple[bytes]) -> None
        data_length = sum(len(block) for block in data_blocks)
        header = struct.pack(HEADER_FORMAT, txn_id, command, flags, data_length)
        self.out_file.write(header)
        for block in data_blocks:
            self.out_file.write(block)
        self.out_file.flush()

    def dump_manifest(self, rev, request):
        # type: (str, Request) -> None
        """
        Send the manifest data.
        """
        start = time.time()
        try:
            ctx = self.repo[rev]
            mf = ctx.manifest()
        except Exception:
            # The mercurial call may fail with a "no node" error if this
            # revision in question has added to the repository after we
            # originally opened it.  Invalidate the repository and try again,
            # in case our cached repo data is just stale.
            self.repo.invalidate(clearfilecache=True)
            ctx = self.repo[rev]
            mf = ctx.manifest()

        # How many paths to send in each chunk
        # Empirically, 100 seems like a decent number.
        # Too small and we pay a cost for doing too many small writes.
        # Too big and the C++ code is idle while it waits for us to build a
        # chunk, and then we fill up the pipe writing the data out, and have
        # to wait for it to be processed before we can start building the next
        # chunk.
        MANIFEST_PATHS_PER_CHUNK = 100

        chunked_paths = []
        num_paths = 0
        for path, hashval, flags in mf.iterentries():
            # Construct the chunk data using join(), since that is relatively
            # fast compared to other ways of constructing python strings.
            entry = b"\t".join((hashval, flags, path + b"\0"))
            if len(chunked_paths) >= MANIFEST_PATHS_PER_CHUNK:
                num_paths += len(chunked_paths)
                self.send_chunk(request, b"".join(chunked_paths), is_last=False)
                chunked_paths = [entry]
            else:
                chunked_paths.append(entry)

        num_paths += len(chunked_paths)
        self.send_chunk(request, b"".join(chunked_paths), is_last=True)
        self.debug(
            "sent manifest with %d paths in %s seconds", num_paths, time.time() - start
        )

    def _get_manifest_node_impl(self, rev):
        # type: (str) -> bytes
        ctx = self.repo[rev]
        node_hash = ctx.manifestnode()
        if not node_hash:
            # For some reason ctx.manifestnode() can sometimes successfully
            # return an empty string.  This does seem like a cache invalidation
            # bug somehow, as it behaves correctly after restarting the
            # process.  Ideally this broken behavior in mercurial should be
            # fixed. For now translate this into an exception so we will retry
            # after invalidating the cache.
            raise RuntimeError(
                "mercurial bug: ctx.manifestnode() returned an "
                "empty string for commit %s" % (rev,)
            )
        return node_hash

    def get_manifest_node(self, rev):
        # type: (str) -> bytes
        try:
            return self._get_manifest_node_impl(rev)
        except Exception:
            # The mercurial call may fail with a "no node" error if this
            # revision in question has added to the repository after we
            # originally opened it.  Invalidate the repository and try again,
            # in case our cached repo data is just stale.
            #
            # clearfilecache=True is necessary so that mercurial will open
            # 00changelog.i.a if it exists now instead of just using
            # 00changelog.i  The .a file contains pending commit data if a
            # transaction is in progress.
            if self.repo.ui.configbool("devel", "all-warnings"):
                logging.exception("Got exception when getting manifest node: %s", rev)
            self.repo.invalidate(clearfilecache=True)
            return self._get_manifest_node_impl(rev)

    def get_file(self, path, rev_hash):
        # type: (str, bytes) -> bytes
        return self.get_file_attribute(path, rev_hash, "data", lambda fctx: fctx.data())

    def get_file_size(self, path, rev_hash):
        # type: (str, bytes) -> int
        return self.get_file_attribute(path, rev_hash, "size", lambda fctx: fctx.size())

    def get_file_attribute(self, path, rev_hash, attr, attr_of):
        # type: (str, str, bytes, Callable[[context.filectx], AttributeType])
        #       -> AttributeType
        try:
            fctx = self.repo.filectx(path, fileid=rev_hash)
        except Exception:
            self.repo.invalidate()
            fctx = self.repo.filectx(path, fileid=rev_hash)

        try:
            return attr_of(fctx)
        except KeyError as e:
            # There is a race condition in Mercurial's repacking which may be
            # triggered by debugedenimporthelper since we have multiple
            # processes for importing files. So we will retry once for this
            # type of error to avoid restarting the importer when this happens.
            self.repo.ui.develwarn("Retrying due to possible race condition")

            if not hasattr(self.repo.fileslog, "contentstore"):
                # we are not using remotefilelog in tests, and normal filelog
                # does not have a contentstore. So fail immediately
                raise ResetRepoError(e)

            # Committing the fileslog will force the contentstore to be
            # rebuilt, effectively refreshing the store.
            self.repo.fileslog.commitpending()

            try:
                return attr_of(fctx)
            except Exception as e:
                raise ResetRepoError(e)
        except Exception as e:
            # Ugh.  The server-side remotefilelog code can sometimes
            # incorrectly fail to return data here.  I believe this occurs if
            # the file data is new since we first opened our connection to the
            # server.
            #
            # Completely re-initialize our repo object and try again, in hopes
            # that this will make the server return data correctly when we
            # retry.
            if self.repo.ui.configbool("devel", "all-warnings"):
                logging.exception(
                    "Exception while getting file %s: %s, %s", attr, path, hex(rev_hash)
                )
            raise ResetRepoError(e)

    # pyre-fixme[56]: While applying decorator
    #  `edenscm.mercurial.commands.eden.cmd(...)`: Expected `(Request) -> None` for 1st
    #  param but got `(self: HgServer, request: Request) -> None`.
    @cmd(CMD_PREFETCH_FILES)
    def prefetch_files(self, request):
        # type: (Request) -> None
        self._do_prefetch(request)
        self.send_chunk(request, b"")

    def _do_prefetch(self, request):
        # type: (Request) -> None
        # Some repos may not have remotefilelog enabled; for example,
        # the watchman integration tests have no remote server and no
        # remotefilelog.
        if not util.safehasattr(self.repo, "fileservice"):
            logging.debug("ignoring prefetch request in non-remotefilelog repository")
            return

        logging.debug("got prefetch request, parsing")
        [num_files] = struct.unpack_from(b">I", request.body, 0)
        if num_files > 4000000:
            # Ignore requests with an extremely large number of files to prefetch,
            # to prevent us from consuming logs of memory and CPU trying to deserialize
            # garbage data.
            #
            # This is likely a request from an older edenfs daemon that sends JSON data
            # here rather than our binary serialization format.  We just return a
            # successful response and ignore the request in this case rather than
            # responding with an error.  Responding with an error will cause these older
            # edenfs versions to propagate the error back to clients in some cases;
            # ignoring the request will allow things to proceed normally, but just
            # slower than if the data had been prefetched.
            logging.debug(
                "ignoring prefetch request with too many files: %r", num_files
            )
            return
        offset = 4  # struct.calcsize(">I")
        lengths_fmt = b">" + (num_files * b"I")
        path_lengths = struct.unpack_from(lengths_fmt, request.body, offset)
        offset += num_files * 4  # struct.calcsize(lengths_fmt)
        data_fmt = b"".join(b"%ds40s" % length for length in path_lengths)
        files_data = struct.unpack_from(data_fmt, request.body, offset)

        files = []
        for n in range(num_files):
            idx = n * 2
            files.append((pycompat.decodeutf8(files_data[idx]), files_data[idx + 1]))

        logging.debug("will prefetch %d files" % len(files))
        self.repo.fileservice.prefetch(files)


def always_allow_pending(root):
    # type: (bytes) -> bool
    return True


def always_allow_shared_pending(root, sharedroot):
    # type: (bytes, bytes) -> bool
    return True


def _open_repo(orig_ui, repo_path):
    # type: (ui.ui, str) -> localrepo.localrepository
    ui = HgUI.load()

    for section, name, value in orig_ui.walkconfig():
        if orig_ui.configsource(section, name) == "--config":
            ui.setconfig(section, name, value, source="--config")

    # Create a fresh copy of the UI object, and load the repository's
    # config into it.  Then load extensions specified by this config.
    hgrc = os.path.join(repo_path, ".hg", "hgrc")
    local_ui = ui.copy()
    local_ui.readconfig(hgrc, repo_path)
    extensions.loadall(local_ui)

    # Create the repository using the original clean UI object that has not
    # loaded the repo config yet.  This is required to ensure that
    # secondary repository objects end up with the correct configuration,
    # and do not have configuration settings from this repository.
    #
    # Secondary repo objects can be created mainly happens due to the share
    # extension.  In general the repository we are pointing at should
    # should not itself point to another shared repo, but it seems safest
    # to exactly mimic mercurial's own start-up behavior here.
    repo_ui = ui.copy()
    return hg.repository(repo_ui, repo_path)


def runedenimporthelper(repo, **opts):
    # type: (localrepo.localrepository, str) -> int
    fd = opts.get("in_fd")
    in_fd = int(fd) if fd else None
    fd = opts.get("out_fd")
    out_fd = int(fd) if fd else None
    server = HgServer(repo, in_fd=in_fd, out_fd=out_fd)

    manifest_rev = opts.get("get_manifest_node")
    if manifest_rev:
        node = server.get_manifest_node(manifest_rev)
        repo.ui.write(hex(node) + "\n")
        return 0

    manifest_arg = opts.get("manifest")
    if manifest_arg:
        manifest_rev = bin(manifest_arg)
        request = Request(0, CMD_MANIFEST, flags=0, body=manifest_rev)
        server.dump_manifest(manifest_arg, request)
        return 0

    cat_file_arg = opts.get("cat_file")
    if cat_file_arg:
        path, file_rev_str = cat_file_arg.rsplit(":", -1)
        file_rev = bin(file_rev_str)
        data = server.get_file(path, file_rev)
        repo.ui.writebytes(data)
        return 0

    cat_manifest_arg = opts.get("cat_tree")
    if cat_manifest_arg:
        path, manifest_node_str = cat_manifest_arg.rsplit(":", -1)
        manifest_node = bin(manifest_node_str)
        data = server.get_tree(path, manifest_node)
        repo.ui.writebytes(data)
        return 0

    get_file_size_arg = opts.get("get_file_size")
    if get_file_size_arg:
        path, id_str = get_file_size_arg.rsplit(":", -1)
        id = bin(id_str)
        size = server.get_file_size(path, id)
        repo.ui.write("{}\n".format(size))
        return 0

    fetch_tree_arg = opts.get("fetch_tree")
    if fetch_tree_arg:
        parts = fetch_tree_arg.rsplit(":", -1)
        if len(parts) == 1:
            path = parts[0]
            if path == "":
                manifest_node = server.get_manifest_node(".")
            else:
                # TODO: It would be nice to automatically look up the current
                # manifest node ID for this path and use that here, assuming
                # we have sufficient data locally for this
                raise RuntimeError("a manifest node ID is required when using a path")
        else:
            path, manifest_node_str = parts
            manifest_node = bin(manifest_node_str)
            if len(manifest_node) != 20:
                raise RuntimeError("manifest node should be a 40-byte hex string")

        server.fetch_tree(path, manifest_node)
        return 0

    # If one of the above debug options wasn't used, require the --out-fd flag.
    # This flag is required to ensure that other mercurial code that prints to stdout
    # cannot interfere with our output.
    if not opts.get("out_fd"):
        raise error.Abort(_("the --out-fd argument is required"))

    try:
        return server.serve()
    except KeyboardInterrupt:
        logging.debug("hg_import_helper received interrupt; shutting down")
        return 0


# pyre-fixme[56]: Argument `[("", "in-fd", "", edenscm.mercurial.i18n._("Use the
#  specified file descriptor to receive commands, rather than reading on stdin"),
#  edenscm.mercurial.i18n._("FILENO")), ("", "out-fd", "",
#  edenscm.mercurial.i18n._("Use the specified file descriptor to send command output,
#  rather than writing to stdout"), edenscm.mercurial.i18n._("FILENO")), ("",
#  "manifest", "", edenscm.mercurial.i18n._("Dump the binary manifest data for the
#  specified revision."), edenscm.mercurial.i18n._("REVISION")), ("",
#  "get-manifest-node", "", edenscm.mercurial.i18n._("Print the manifest node ID for
#  the specified revision."), edenscm.mercurial.i18n._("REVISION")), ("", "cat-file",
#  "", edenscm.mercurial.i18n._("Dump the file contents for the specified file at the
#  given file revision"), edenscm.mercurial.i18n._("PATH:REV")), ("", "get-file-size",
#  "", edenscm.mercurial.i18n._("Get the file size for the specified file at the given
#  file revision"), edenscm.mercurial.i18n._("PATH:REV")), ("", "fetch-tree", "",
#  edenscm.mercurial.i18n._("Fetch treemanifest data for the specified path at the
#  given manifest node"), edenscm.mercurial.i18n._("PATH:REV"))]` to decorator factory
#  `edenscm.mercurial.commands.cmdtable.command` could not be resolved in a global
#  scope.
@command(
    "debugedenimporthelper",
    [
        (
            "",
            "in-fd",
            "",
            _(
                "Use the specified file descriptor to receive commands, rather than reading on stdin"
            ),
            _("FILENO"),
        ),
        (
            "",
            "out-fd",
            "",
            _(
                "Use the specified file descriptor to send command output, rather than writing to stdout"
            ),
            _("FILENO"),
        ),
        (
            "",
            "manifest",
            "",
            _("Dump the binary manifest data for the specified revision."),
            _("REVISION"),
        ),
        (
            "",
            "get-manifest-node",
            "",
            _("Print the manifest node ID for the specified revision."),
            _("REVISION"),
        ),
        (
            "",
            "cat-file",
            "",
            _(
                "Dump the file contents for the specified file at the given file revision"
            ),
            _("PATH:REV"),
        ),
        (
            "",
            "cat-tree",
            "",
            _(
                "Dump the tree contents for the specified path at the given manifest node"
            ),
            _("PATH:NODE"),
        ),
        (
            "",
            "get-file-size",
            "",
            _("Get the file size for the specified file at the given file revision"),
            _("PATH:REV"),
        ),
        (
            "",
            "fetch-tree",
            "",
            _(
                "Fetch treemanifest data for the specified path at the given manifest node"
            ),
            _("PATH:REV"),
        ),
    ],
    _("[REPO]"),
    optionalrepo=True,
)
def eden_import_helper(ui, repo, *repo_args, **opts):
    # type: (ui.ui, Optional[localrepo.localrepository], str, str) -> int
    """Obtain data for edenfs"""
    logging.basicConfig(
        stream=sys.stderr, level=logging.INFO, format="%(asctime)s %(message)s"
    )

    # We always want to be able to access commits being created by pending
    # transactions.
    #
    # Monkey-patch mercural.txnutil and replace its _mayhavepending() function
    # with one that always returns True.  We could set the HG_PENDING
    # environment variable to try and get it to return True without
    # monkey-patching, but this seems a bit more fragile--it requires an exact
    # string match on the repository path, so we would have to make sure to
    # normalize the repository path the same way mercurial does, and make sure
    # we use the correct repository (in case of a shared repository).
    txnutil.mayhavepending = always_allow_pending
    txnutil.mayhavesharedpending = always_allow_shared_pending

    openedrepo = False
    if len(repo_args) > 1:
        raise error.Abort(_("only 1 repository path argument is allowed"))
    elif len(repo_args) == 1:
        # If a repository path is explicitly specified prefer that over the normal
        # repository selected by the mercurial repository.  This is for backwards
        # compatibility with old edenfs processes that didn't use the normal hg repo
        # path arguments.
        repo = _open_repo(ui, repo_args[0])
        openedrepo = True
    elif repo is None:
        raise error.Abort(_("no repository specified"))

    # debugedenimporthelper is one of the few commands run in the backing
    # repository. In order to keep that repo up-to-date we need to migrate it.
    # Most repos get this when running pull, but pull is never run in the
    # backing repo.
    if ui.configbool("edenfs", "automigrate"):
        repo.automigratestart()

    try:
        return runedenimporthelper(repo, **opts)
    finally:
        # If the repo wasn't passed through -R, we need to close it to clean it
        # up properly.
        if openedrepo:
            repo.close()


# pyre-fixme[56]: Argument `[]` to decorator factory
#  `edenscm.mercurial.commands.cmdtable.command` could not be resolved in a global
#  scope.
@command("debugedenrunpostupdatehook", [])
def edenrunpostupdatehook(ui, repo):
    # type: (ui.ui, localrepo.localrepository) -> None
    """Run post-update hooks for edenfs"""
    with repo.wlock():
        parent1, parent2 = ([hex(node) for node in repo.nodes("parents()")] + ["", ""])[
            :2
        ]
        repo.hook("preupdate", throw=False, parent1=parent1, parent2=parent2)
        repo.hook("update", parent1=parent1, parent2=parent2, error=0)
