#!/usr/bin/env python2
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import argparse
import binascii
import collections
import logging
import os
import struct
import sys
import time


try:
    import edenscm.mercurial as mercurial
    import edenscm.mercurial.error
    import edenscm.mercurial.hg
    import edenscm.mercurial.node
    import edenscm.mercurial.scmutil
    import edenscm.mercurial.txnutil
    import edenscm.mercurial.ui
    import edenscm.mercurial.util
except ImportError:
    import mercurial.error  # @manual
    import mercurial.hg  # @manual
    import mercurial.node  # @manual
    import mercurial.scmutil  # @manual
    import mercurial.txnutil  # @manual
    import mercurial.ui  # @manual
    import mercurial.util  # @manual


if os.name == "nt":
    from msvcrt import open_osfhandle  # @manual

    def fdopen(handle, mode):
        os_mode = os.O_WRONLY if mode == "wb" else os.O_RDONLY
        fileno = open_osfhandle(handle, os_mode)
        return os.fdopen(fileno, mode)


else:

    def fdopen(handle, mode):
        return os.fdopen(handle, mode)


hex = binascii.hexlify


try:
    from edenscm.hgext.remotefilelog import shallowutil, constants
except ImportError:
    from hgext.remotefilelog import shallowutil, constants  # @manual

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
        super(ResetRepoError, self).__init__(
            "hg repository needs to be re-opened: %s" % (original_error,)
        )


def cmd(command_id):
    """
    A helper function for identifying command functions
    """

    def decorator(func):
        func.__COMMAND_ID__ = command_id
        return func

    return decorator


class HgUI(mercurial.ui.ui):
    def __init__(self, src=None):
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
        return True

    def interactive(self):
        return False


class HgServer(object):
    def __init__(self, repo_path, config_overrides, in_fd=None, out_fd=None):
        """
        Create an HgServer.

        repo_path:
          The path to the mercurial repository
        config_overrides:
          A list of ConfigOption values, to be passed to ui.setconfig() when
          initializing the mercurial UI, after loading the normal config
          settings.  This is equivalent to specifying config options on the
          mercurial command line with `--config section.name=value`
        in_fd:
          A file descriptor to use for receiving requests.
          If in_fd is None, stdin will be used.
        out_fd:
          A file descriptor to use for sending responses.
          If in_fd is None, stdout will be used.
        """
        self.repo_path = repo_path
        self.config_overrides = config_overrides
        if in_fd is None:
            self.in_file = sys.stdin
        else:
            self.in_file = fdopen(in_fd, "rb")
        if out_fd is None:
            self.out_file = sys.stdout
        else:
            self.out_file = fdopen(out_fd, "wb")

        # The repository will be set during initialized()
        self.repo = None
        self.ui = None

        # Populate our command dictionary
        self._commands = {}
        for member_name in dir(self):
            value = getattr(self, member_name)
            if not hasattr(value, "__COMMAND_ID__"):
                continue
            self._commands[value.__COMMAND_ID__] = value

    def initialize(self):
        self.ui = HgUI.load()
        for opt in self.config_overrides:
            self.ui.setconfig(opt.section, opt.name, opt.value, source="--config")

        # Create a fresh copy of the UI object, and load the repository's
        # config into it.  Then load extensions specified by this config.
        hgrc = os.path.join(self.repo_path, b".hg", b"hgrc")
        local_ui = self.ui.copy()
        local_ui.readconfig(hgrc, self.repo_path)
        mercurial.extensions.loadall(local_ui)

        self.repo = self._open_repo()

        try:
            self.treemanifest = mercurial.extensions.find("treemanifest")
        except KeyError:
            # The treemanifest extension is not present
            self.treemanifest = None

    def _open_repo(self):
        # Create the repository using the original clean UI object that has not
        # loaded the repo config yet.  This is required to ensure that
        # secondary repository objects end up with the correct configuration,
        # and do not have configuration settings from this repository.
        #
        # Secondary repo objects can be created mainly happens due to the share
        # extension.  In general the repository we are pointing at should
        # should not itself point to another shared repo, but it seems safest
        # to exactly mimic mercurial's own start-up behavior here.
        repo_ui = self.ui.copy()
        repo = mercurial.hg.repository(repo_ui, self.repo_path)
        return repo.unfiltered()

    def _reopen_repo(self):
        # Close the current repo and make a new one.
        # We use this to deal with invalidation related errors that are
        # more likely to bubble to the surface with our long lived use case.

        # Reset self.repo to None before we try to close and re-open it,
        # so that if anything goes wrong it will be None rather than still pointing to a
        # partially closed repository.
        repo = self.repo
        self.repo = None

        try:
            repo.close()
        except Exception as ex:
            logging.warning("error closing repository: %s" % (ex,))

        self.repo = self._open_repo()

    def serve(self):
        try:
            self.initialize()
        except Exception as ex:
            # If an error occurs during initialization (say, if the repository
            # path is invalid), send an error response.
            self.send_exception(request=None, exc=ex)
            return 1

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
        return name in ["fbsource", "www"]

    def _gen_options(self):
        repo_name = getattr(self.repo, "name", None)
        use_treemanifest = (self.treemanifest is not None) and bool(repo_name)
        use_mononoke = use_treemanifest and self._is_mononoke_supported(repo_name)

        flags = 0
        treemanifest_paths = []
        if use_treemanifest:
            flags |= START_FLAGS_TREEMANIFEST_SUPPORTED
            treemanifest_paths = [
                shallowutil.getlocalpackpath(
                    self.repo.svfs.vfs.base, constants.TREEPACK_CATEGORY
                ),
                shallowutil.getcachepackpath(self.repo, constants.TREEPACK_CATEGORY),
                shallowutil.getlocalpackpath(
                    self.repo.svfs.vfs.base, constants.FILEPACK_CATEGORY
                ),
                shallowutil.getcachepackpath(self.repo, constants.FILEPACK_CATEGORY),
            ]

        if use_mononoke:
            flags |= START_FLAGS_MONONOKE_SUPPORTED

        # Options format:
        # - Protocol version number
        # - Is treemanifest supported?
        # - Number of treemanifest paths
        #   - treemanifest paths, encoded as (length, string_data)
        parts = []
        parts.append(
            struct.pack(b">III", PROTOCOL_VERSION, flags, len(treemanifest_paths))
        )
        for path in treemanifest_paths:
            parts.append(struct.pack(b">I", len(path)))
            parts.append(path)

        if use_mononoke:
            parts.append(struct.pack(b">I", len(repo_name)))
            parts.append(repo_name)

        return "".join(parts)

    def debug(self, msg, *args, **kwargs):
        logging.debug(msg, *args, **kwargs)

    def process_request(self):
        # Read the request header
        header_data = self.in_file.read(HEADER_SIZE)
        if not header_data:
            # EOF.  All done serving
            return False

        if len(header_data) < HEADER_SIZE:
            raise Exception("received EOF after partial request header")

        header_fields = struct.unpack(HEADER_FORMAT, header_data)
        txn_id, command, flags, data_len = header_fields

        # Read the request body
        body = self.in_file.read(data_len)
        if len(body) < data_len:
            raise Exception("received EOF after partial request")
        req = Request(txn_id, command, flags, body)

        cmd_function = self._commands.get(command)
        if cmd_function is None:
            logging.warning("unknown command %r", command)
            self.send_error(req, "CommandError", "unknown command %r" % (command,))
            return True

        try:
            # Ensure that the repository is open.  It may be None here if something
            # went wrong previously trying to re-open the repository during a previous
            # command.
            if self.repo is None:
                self.repo = self._open_repo()

            cmd_function(req)
        except Exception as ex:
            logging.exception("error processing command %r", command)
            self.send_exception(req, ex)

        # Return True to indicate that we should continue serving
        return True

    @cmd(CMD_MANIFEST)
    def cmd_manifest(self, request):
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
        rev_name = request.body
        self.debug("sending manifest for revision %r", rev_name)
        self.dump_manifest(rev_name, request)

    @cmd(CMD_OLD_CAT_FILE)
    def cmd_old_cat_file(self, request):
        """Handler for CMD_OLD_CAT_FILE requests.

        This requests the contents for a given file.
        New edenfs servers do not send this request, but we still need to support this
        for old edenfs servers that have not been restarted.

        This is similar to CMD_CAT_FILE, but the response body contains only the raw
        file contents.
        """
        if len(request.body) < SHA1_NUM_BYTES + 1:
            raise Exception("old_cat_file request data too short")

        rev_hash = request.body[:SHA1_NUM_BYTES]
        path = request.body[SHA1_NUM_BYTES:]
        self.debug(
            "(pid:%s) CMD_OLD_CAT_FILE request for contents of file %r revision %s",
            os.getpid(),
            path,
            hex(rev_hash),
        )

        contents = self.get_file(path, rev_hash)
        self.send_chunk(request, contents)

    @cmd(CMD_CAT_FILE)
    def cmd_cat_file(self, request):
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
            raise Exception("cat_file request data too short")

        rev_hash = request.body[:SHA1_NUM_BYTES]
        path = request.body[SHA1_NUM_BYTES:]
        self.debug(
            "(pid:%s) getting contents of file %r revision %s",
            os.getpid(),
            path,
            hex(rev_hash),
        )

        contents = self.get_file(path, rev_hash)
        length_data = struct.pack(b">Q", len(contents))
        self.send_chunk(request, contents, length_data)

    @cmd(CMD_MANIFEST_NODE_FOR_COMMIT)
    def cmd_manifest_node_for_commit(self, request):
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
        rev_name = request.body
        self.debug("resolving manifest node for revision %r", rev_name)
        try:
            node = self.get_manifest_node(rev_name)
        except mercurial.error.RepoError as ex:
            # Handle lookup errors explicitly, just so we avoid printing
            # a backtrace in the log if we let this bubble all the way up
            # to the unexpected exception handling code in process_request()
            self.send_exception(request, ex)
            return

        self.send_chunk(request, node)

    @cmd(CMD_FETCH_TREE)
    def cmd_fetch_tree(self, request):
        if len(request.body) < SHA1_NUM_BYTES:
            raise Exception(
                "fetch_tree request data too short: len=%d" % len(request.body)
            )

        manifest_node = request.body[:SHA1_NUM_BYTES]
        path = request.body[SHA1_NUM_BYTES:]
        self.debug(
            "fetching tree for path %r manifest node %s", path, hex(manifest_node)
        )

        self.fetch_tree(path, manifest_node)
        self.send_chunk(request, b"")

    def fetch_tree(self, path, manifest_node):
        if self.treemanifest is None:
            raise Exception("treemanifest not enabled in this repository")

        try:
            self._fetch_tree_impl(path, manifest_node)
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
        mfnodes = set([manifest_node])

        # It would be nice to initially only fetch the one tree that we need
        # immediately, and fetch the rest of the subtree later, in the
        # background.  Unfortunately the wire protocol API does not support a
        # mechanism to do this yet.  In the future it's probably worth adding a
        # "depth" parameter requesting data only down to a specific depth.

        if path:
            # We have to call repo._prefetchtrees() directly if we have a path.
            # We cannot compute the set of base nodes in this case.
            self.repo._prefetchtrees(path, mfnodes, [], [])
            self.repo.manifestlog.commitpending()
        else:
            # When querying the top-level node use repo.prefetchtrees()
            # It will compute a reasonable set of base nodes to send in the query.
            self.repo.prefetchtrees(mfnodes)
            self.repo.manifestlog.commitpending()

    def send_chunk(self, request, *data, **kwargs):
        is_last = kwargs.pop("is_last", True)
        if kwargs:
            raise TypeError("unexpected keyword arguments: %r" % (kwargs.keys(),))

        flags = 0
        if not is_last:
            flags |= FLAG_MORE_CHUNKS

        self._send_chunk(
            request.txn_id, command=CMD_RESPONSE, flags=flags, data_blocks=data
        )

    def send_exception(self, request, exc):
        self.send_error(request, type(exc).__name__, str(exc))

    def send_error(self, request, error_type, message):
        txn_id = 0
        if request is not None:
            txn_id = request.txn_id

        data = b"".join(
            [
                struct.pack(b">I", len(error_type)),
                error_type,
                struct.pack(b">I", len(message)),
                message,
            ]
        )
        self._send_chunk(
            txn_id, command=CMD_RESPONSE, flags=FLAG_ERROR, data_blocks=(data,)
        )

    def _send_chunk(self, txn_id, command, flags, data_blocks):
        data_length = sum(len(block) for block in data_blocks)
        header = struct.pack(HEADER_FORMAT, txn_id, command, flags, data_length)
        self.out_file.write(header)
        for block in data_blocks:
            self.out_file.write(block)
        self.out_file.flush()

    def dump_manifest(self, rev, request):
        """
        Send the manifest data.
        """
        start = time.time()
        try:
            ctx = mercurial.scmutil.revsingle(self.repo, rev)
            mf = ctx.manifest()
        except Exception:
            # The mercurial call may fail with a "no node" error if this
            # revision in question has added to the repository after we
            # originally opened it.  Invalidate the repository and try again,
            # in case our cached repo data is just stale.
            self.repo.invalidate(clearfilecache=True)
            ctx = mercurial.scmutil.revsingle(self.repo, rev)
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
        ctx = mercurial.scmutil.revsingle(self.repo, rev)
        node_hash = ctx.manifestnode()
        if not node_hash:
            # For some reason ctx.manifestnode() can sometimes successfully
            # return an empty string.  This does seem like a cache invalidation
            # bug somehow, as it behaves correctly after restarting the
            # process.  Ideally this broken behavior in mercurial should be
            # fixed. For now translate this into an exception so we will retry
            # after invalidating the cache.
            raise Exception(
                "mercurial bug: ctx.manifestnode() returned an "
                "empty string for commit %s" % (rev,)
            )
        return node_hash

    def get_manifest_node(self, rev):
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
            self.repo.invalidate(clearfilecache=True)
            return self._get_manifest_node_impl(rev)

    def get_file(self, path, rev_hash):
        try:
            fctx = self.repo.filectx(path, fileid=rev_hash)
        except Exception:
            self.repo.invalidate()
            fctx = self.repo.filectx(path, fileid=rev_hash)

        try:
            return fctx.data()
        except Exception as ex:
            # Ugh.  The server-side remotefilelog code can sometimes
            # incorrectly fail to return data here.  I believe this occurs if
            # the file data is new since we first opened our connection to the
            # server.
            #
            # Completely re-initialize our repo object and try again, in hopes
            # that this will make the server return data correctly when we
            # retry.
            raise ResetRepoError(ex)

    def prefetch(self, rev):
        if not hasattr(self.repo, "prefetch"):
            # This repo isn't using remotefilelog, so nothing to do.
            return

        try:
            rev_range = mercurial.scmutil.revrange(self.repo, rev)
        except Exception:
            self.repo.invalidate()
            rev_range = mercurial.scmutil.revrange(self.repo, rev)

        self.debug("prefetching")
        self.repo.prefetch(rev_range)
        self.debug("done prefetching")

    @cmd(CMD_PREFETCH_FILES)
    def prefetch_files(self, request):
        self._do_prefetch(request)
        self.send_chunk(request, "")

    def _do_prefetch(self, request):
        # Some repos may not have remotefilelog enabled; for example,
        # the watchman integration tests have no remote server and no
        # remotefilelog.
        if not hasattr(self.repo, "fileservice"):
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
            files.append((files_data[idx], files_data[idx + 1]))

        logging.debug("will prefetch %d files" % len(files))
        self.repo.fileservice.prefetch(files)


def always_allow_pending(root):
    return True


def always_allow_shared_pending(root, sharedroot):
    return True


ConfigOption = collections.namedtuple("ConfigOption", ["section", "name", "value"])


def parse_config_options(argparser, options):
    """
    Parse config options specified using --config arguments.

    The options parameter should be the list of --config option values.
    Each option value should be of the form "section.name=value"

    This function returns a list of ConfigOption objects.
    """
    results = []
    for option in options:
        try:
            name, value = [element.strip() for element in option.split("=", 1)]
            section, name = name.split(".", 1)
            results.append(ConfigOption(section, name, value))
        except (IndexError, ValueError):
            argparser.error(
                "bad --config argument %r: must be of the form "
                "SECTION.NAME=VALUE" % (option,)
            )
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("repo", help="The repository path")
    parser.add_argument(
        "--config",
        metavar="SECTION.NAME=VALUE",
        action="append",
        default=[],
        help="Specify mercurial configuration options",
    )
    parser.add_argument(
        "--in-fd",
        metavar="FILENO",
        type=int,
        help="Use the specified file descriptor to receive "
        "commands, rather than reading on stdin",
    )
    parser.add_argument(
        "--out-fd",
        metavar="FILENO",
        type=int,
        help="Use the specified file descriptor to send "
        "command output, rather than writing to stdout",
    )

    # Arguments for testing and debugging.
    # These cause the helper to perform a single operation and exit,
    # rather than running as a server.
    parser.add_argument(
        "--manifest",
        metavar="REVISION",
        help="Dump the binary manifest data for the specified " "revision.",
    )
    parser.add_argument(
        "--get-manifest-node",
        metavar="REVISION",
        help="Print the manifest node ID for the specified " "revision.",
    )
    parser.add_argument(
        "--cat-file",
        metavar="PATH:REV",
        help="Dump the file contents for the specified file "
        "at the given file revision",
    )
    parser.add_argument(
        "--fetch-tree",
        metavar="PATH:REV",
        help="Fetch treemanifest data for the specified path "
        "at the given manifest node",
    )

    args = parser.parse_args()
    config_overrides = parse_config_options(parser, args.config)

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
    mercurial.txnutil.mayhavepending = always_allow_pending
    mercurial.txnutil.mayhavesharedpending = always_allow_shared_pending

    server = HgServer(args.repo, config_overrides, in_fd=args.in_fd, out_fd=args.out_fd)

    if args.get_manifest_node:
        server.initialize()
        node = server.get_manifest_node(args.get_manifest_node)
        print(hex(node))
        return 0

    if args.manifest is not None:
        server.initialize()
        request = Request(0, CMD_MANIFEST, flags=0, body=args.manifest)
        server.dump_manifest(args.manifest, request)
        return 0

    if args.cat_file is not None:
        server.initialize()
        path, file_rev_str = args.cat_file.rsplit(":", -1)
        path = path.encode(sys.getfilesystemencoding())
        file_rev = binascii.unhexlify(file_rev_str)
        data = server.get_file(path, file_rev)
        sys.stdout.write(data)
        return 0

    if args.fetch_tree is not None:
        server.initialize()
        parts = args.fetch_tree.rsplit(":", -1)
        if len(parts) == 1:
            path = parts[0]
            if path == "":
                manifest_node = server.get_manifest_node(".")
            else:
                # TODO: It would be nice to automatically look up the current
                # manifest node ID for this path and use that here, assuming
                # we have sufficient data locally for this
                raise Exception("a manifest node ID is required when " "using a path")
        else:
            path, manifest_node_str = parts
            manifest_node = binascii.unhexlify(manifest_node_str)
            if len(manifest_node) != 20:
                raise Exception("manifest node should be a 40-byte hex string")

        server.fetch_tree(path, manifest_node)
        return 0

    try:
        return server.serve()
    except KeyboardInterrupt:
        logging.debug("hg_import_helper received interrupt; shutting down")


if __name__ == "__main__":
    rc = main()
    sys.exit(rc)
