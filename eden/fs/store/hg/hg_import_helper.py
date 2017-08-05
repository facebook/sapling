#!/usr/bin/env python2
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import argparse
import binascii
import collections
import logging
import os
import struct
import sys
import time

import mercurial.error
import mercurial.hg
import mercurial.node
import mercurial.scmutil
import mercurial.txnutil
import mercurial.ui
from remotefilelog import shallowutil, constants

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
HEADER_FORMAT = b'>IIII'
HEADER_SIZE = 16

# The length of a SHA-1 hash
SHA1_NUM_BYTES = 20

#
# Message types.
#
# See the specific cmd_* functions below for documentation on the
# request/response formats.
#
CMD_STARTED = 0
CMD_RESPONSE = 1
CMD_MANIFEST = 2
CMD_CAT_FILE = 3
CMD_MANIFEST_NODE_FOR_COMMIT = 4
CMD_GET_CACHE_PATH = 5
CMD_FETCH_TREE = 6

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


def cmd(command_id):
    '''
    A helper function for identifying command functions
    '''
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

    def interactive(self):
        return False


class HgServer(object):
    def __init__(self, repo_path, config_overrides, in_fd=None, out_fd=None):
        '''
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
        '''
        self.repo_path = repo_path
        self.config_overrides = config_overrides
        if in_fd is None:
            self.in_file = sys.stdin
        else:
            self.in_file = os.fdopen(in_fd, 'rb')
        if out_fd is None:
            self.out_file = sys.stdout
        else:
            self.out_file = os.fdopen(out_fd, 'wb')

        # The repository will be set during initialized()
        self.repo = None
        self.ui = None

        # Populate our command dictionary
        self._commands = {}
        for member_name in dir(self):
            value = getattr(self, member_name)
            if not hasattr(value, '__COMMAND_ID__'):
                continue
            self._commands[value.__COMMAND_ID__] = value

    def initialize(self):
        hgrc = os.path.join(self.repo_path, b".hg", b"hgrc")
        self.ui = HgUI.load()
        self.ui.readconfig(hgrc, self.repo_path)
        for opt in self.config_overrides:
            self.ui.setconfig(opt.section, opt.name, opt.value,
                              source='--config')

        mercurial.extensions.loadall(self.ui)
        repo = mercurial.hg.repository(self.ui, self.repo_path)
        self.repo = repo.unfiltered()

        try:
            self.treemanifest = mercurial.extensions.find('treemanifest')
        except KeyError:
            # The treemanifest extension is not present
            self.treemanifest = None

    def serve(self):
        try:
            self.initialize()
        except Exception as ex:
            # If an error occurs during initialization (say, if the repository
            # path is invalid), send an error response.
            self.send_error(request=None, message=str(ex))
            return 1

        # Send a success response to indicate we have started
        self._send_chunk(txn_id=0, command=CMD_STARTED,
                         flags=0, data='')

        while self.process_request():
            pass

        logging.debug('hg_import_helper shutting down normally')
        return 0

    def debug(self, msg, *args, **kwargs):
        logging.debug(msg, *args, **kwargs)

    def process_request(self):
        # Read the request header
        header_data = self.in_file.read(HEADER_SIZE)
        if not header_data:
            # EOF.  All done serving
            return False

        if len(header_data) < HEADER_SIZE:
            raise Exception('received EOF after partial request header')

        header_fields = struct.unpack(HEADER_FORMAT, header_data)
        txn_id, command, flags, data_len = header_fields

        # Read the request body
        body = self.in_file.read(data_len)
        if len(body) < data_len:
            raise Exception('received EOF after partial request')
        req = Request(txn_id, command, flags, body)

        cmd_function = self._commands.get(command)
        if cmd_function is None:
            logging.warning('unknown command %r', command)
            self.send_error(req, 'unknown command %r' % (command,))
            return True

        try:
            cmd_function(req)
        except Exception as ex:
            logging.exception('error processing command %r', command)
            self.send_error(req, str(ex))

        # Return True to indicate that we should continue serving
        return True

    @cmd(CMD_MANIFEST)
    def cmd_manifest(self, request):
        '''
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
        '''
        rev_name = request.body
        self.debug('sending manifest for revision %r', rev_name)
        self.dump_manifest(rev_name, request)

    @cmd(CMD_CAT_FILE)
    def cmd_cat_file(self, request):
        '''
        Handler for CMD_CAT_FILE requests.

        This requests the contents for a given file.

        Request body format:
        - <rev_hash><path>
          Fields:
          - <rev_hash>: The file revision hash, as a 20-byte binary value.
          - <path>: The file path, relative to the root of the repository.

        Response body format:
        - <file_contents>
          The body consists solely of the raw file contents.
        '''
        if len(request.body) < SHA1_NUM_BYTES + 1:
            raise Exception('cat_file request data too short')

        rev_hash = request.body[:SHA1_NUM_BYTES]
        path = request.body[SHA1_NUM_BYTES:]
        self.debug('getting contents of file %r revision %s', path,
                   binascii.hexlify(rev_hash))

        contents = self.get_file(path, rev_hash)
        self.send_chunk(request, contents)

    @cmd(CMD_MANIFEST_NODE_FOR_COMMIT)
    def cmd_manifest_node_for_commit(self, request):
        '''
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
        '''
        rev_name = request.body
        self.debug('resolving manifest node for revision %r', rev_name)
        try:
            node = self.get_manifest_node(rev_name)
        except mercurial.error.RepoError as ex:
            # Handle lookup errors explicitly, just so we avoid printing
            # a backtrace in the log if we let this bubble all the way up
            # to the unexpected exception handling code in process_request()
            self.send_error(request, str(ex))
            return

        self.send_chunk(request, node)

    @cmd(CMD_GET_CACHE_PATH)
    def cmd_get_cache_path(self, request):
        '''
        Handler for CMD_GET_CACHE_PATH requests.

        Computes the tree pack cache path for the repo.

        Request body format: no arguments.

        Response body format:
        - The path holding tree packs (string)
        '''
        if not hasattr(self.repo, 'name'):
            # The repo doesn't have the appropriate extensions configured
            # to support tree manifests, so return an empty path.
            # This happens in our integration test suite.
            cache_path = ''
        else:
            cache_path = shallowutil.getcachepackpath(
                self.repo, constants.TREEPACK_CATEGORY)
        self.send_chunk(request, cache_path)

    @cmd(CMD_FETCH_TREE)
    def cmd_fetch_tree(self, request):
        if len(request.body) < SHA1_NUM_BYTES:
            raise Exception('fetch_tree request data too short: len=%d' %
                            len(request.body))

        manifest_node = request.body[:SHA1_NUM_BYTES]
        path = request.body[SHA1_NUM_BYTES:]
        self.debug('fetching tree for path %r manifest node %s',
                   path, binascii.hexlify(manifest_node))

        self.fetch_tree(path, manifest_node)
        self.send_chunk(request, b'')

    def fetch_tree(self, path, manifest_node):
        if self.treemanifest is None:
            raise Exception('treemanifest not enabled in this repository')

        mfnodes = set([manifest_node])
        base_mfnodes = set()

        # The directories parameter isn't actually supported and
        # must always be an empty list.
        directories = []

        # It would be nice to initially only fetch the one tree that we need
        # immediately, and fetch the rest of the subtree later, in the
        # background.  Unfortunately the wire protocol API does not support a
        # mechanism to do this yet.  In the future it's probably worth adding a
        # "depth" parameter requesting data only down to a specific depth.
        self.treemanifest._prefetchtrees(self.repo, path, mfnodes,
                                         base_mfnodes, directories)

    def send_chunk(self, request, data, is_last=True):
        flags = 0
        if not is_last:
            flags |= FLAG_MORE_CHUNKS
        self._send_chunk(request.txn_id, command=CMD_RESPONSE,
                         flags=flags, data=data)

    def send_error(self, request, message):
        txn_id = 0
        if request is not None:
            txn_id = request.txn_id
        self._send_chunk(txn_id, command=CMD_RESPONSE,
                         flags=FLAG_ERROR, data=message)

    def _send_chunk(self, txn_id, command, flags, data):
        header = struct.pack(HEADER_FORMAT, txn_id, command, flags,
                             len(data))
        self.out_file.write(header)
        self.out_file.write(data)
        self.out_file.flush()

    def dump_manifest(self, rev, request):
        '''
        Send the manifest data.
        '''
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
            entry = b'\t'.join((hashval, flags, path + b'\0'))
            if len(chunked_paths) >= MANIFEST_PATHS_PER_CHUNK:
                num_paths += len(chunked_paths)
                self.send_chunk(request, b''.join(chunked_paths),
                                is_last=False)
                chunked_paths = [entry]
            else:
                chunked_paths.append(entry)

        num_paths += len(chunked_paths)
        self.send_chunk(request, b''.join(chunked_paths), is_last=True)
        self.debug('sent manifest with %d paths in %s seconds',
                   num_paths, time.time() - start)

    def get_manifest_node(self, rev):
        try:
            ctx = mercurial.scmutil.revsingle(self.repo, rev)
            return ctx.manifestnode()
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
            ctx = mercurial.scmutil.revsingle(self.repo, rev)
            return ctx.manifestnode()

    def get_file(self, path, rev_hash):
        try:
            fctx = self.repo.filectx(path, fileid=rev_hash)
        except Exception:
            self.repo.invalidate()
            fctx = self.repo.filectx(path, fileid=rev_hash)
        return fctx.data()

    def prefetch(self, rev):
        if not hasattr(self.repo, 'prefetch'):
            # This repo isn't using remotefilelog, so nothing to do.
            return

        try:
            rev_range = mercurial.scmutil.revrange(self.repo, rev)
        except Exception:
            self.repo.invalidate()
            rev_range = mercurial.scmutil.revrange(self.repo, rev)

        self.debug('prefetching')
        self.repo.prefetch(rev_range)
        self.debug('done prefetching')


def always_allow_pending(root):
    return True


ConfigOption = collections.namedtuple('ConfigOption',
                                      ['section', 'name', 'value'])


def parse_config_options(argparser, options):
    '''
    Parse config options specified using --config arguments.

    The options parameter should be the list of --config option values.
    Each option value should be of the form "section.name=value"

    This function returns a list of ConfigOption objects.
    '''
    results = []
    for option in options:
        try:
            name, value = [element.strip() for element in option.split('=', 1)]
            section, name = name.split('.', 1)
            results.append(ConfigOption(section, name, value))
        except (IndexError, ValueError):
            argparser.error('bad --config argument %r: must be of the form '
                            'SECTION.NAME=VALUE' % (option,))
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('repo', help='The repository path')
    parser.add_argument('--config',
                        metavar='SECTION.NAME=VALUE', action='append',
                        default=[],
                        help='Specify mercurial configuration options')
    parser.add_argument('--in-fd',
                        metavar='FILENO', type=int,
                        help='Use the specified file descriptor to receive '
                        'commands, rather than reading on stdin')
    parser.add_argument('--out-fd',
                        metavar='FILENO', type=int,
                        help='Use the specified file descriptor to send '
                        'command output, rather than writing to stdout')

    # Arguments for testing and debugging.
    # These cause the helper to perform a single operation and exit,
    # rather than running as a server.
    parser.add_argument('--manifest',
                        metavar='REVISION',
                        help='Dump the binary manifest data for the specified '
                        'revision.')
    parser.add_argument('--cat-file',
                        metavar='PATH:REV',
                        help='Dump the file contents for the specified file '
                        'at the given file revision')
    parser.add_argument('--fetch-tree',
                        metavar='PATH:REV',
                        help='Fetch treemanifest data for the specified path '
                        'at the given manifest node')

    args = parser.parse_args()
    config_overrides = parse_config_options(parser, args.config)

    logging.basicConfig(stream=sys.stderr, level=logging.DEBUG,
                        format='%(asctime)s %(message)s')

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

    server = HgServer(args.repo, config_overrides,
                      in_fd=args.in_fd, out_fd=args.out_fd)

    if args.manifest is not None:
        server.initialize()
        request = Request(0, CMD_MANIFEST, flags=0, body=args.manifest)
        server.dump_manifest(args.manifest, request)
        return 0

    if args.cat_file is not None:
        server.initialize()
        path, file_rev_str = args.cat_file.rsplit(':', -1)
        path = path.encode(sys.getfilesystemencoding())
        file_rev = binascii.unhexlify(file_rev_str)
        data = server.get_file(path, file_rev)
        sys.stdout.write(data)
        return 0

    if args.fetch_tree is not None:
        server.initialize()
        parts = args.fetch_tree.rsplit(':', -1)
        if len(parts) == 1:
            path = parts[0]
            if path == '':
                manifest_node = server.get_manifest_node('.')
            else:
                # TODO: It would be nice to automatically look up the current
                # manifest node ID for this path and use that here, assuming
                # we have sufficient data locally for this
                raise Exception('a manifest node ID is required when '
                                'using a path')
        else:
            path, manifest_node_str = parts
            manifest_node = binascii.unhexlify(manifest_node_str)
            if len(manifest_node) != 20:
                raise Exception('manifest node should be a 40-byte hex string')

        server.fetch_tree(path, manifest_node)
        return 0

    try:
        return server.serve()
    except KeyboardInterrupt:
        logging.debug('hg_import_helper received interrupt; shutting down')


if __name__ == '__main__':
    rc = main()
    sys.exit(rc)
