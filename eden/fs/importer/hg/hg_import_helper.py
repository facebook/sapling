#!/usr/bin/env python2
#
# Copyright (c) 2016, Facebook, Inc.
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
import logging
import os
import struct
import sys
import time

import mercurial.hg
import mercurial.node
import mercurial.scmutil
import mercurial.ui

FLAG_ERROR = 0x01
FLAG_MORE_CHUNKS = 0x02

# Format argument for struct.unpack()
HEADER_FORMAT = b'>IIII'
HEADER_SIZE = 16

CMD_RESPONSE = 0
CMD_MANIFEST = 1


class HgServer(object):
    def __init__(self, repo_path):
        self.repo_path = repo_path

        hgrc = os.path.join(repo_path, b".hg", b"hgrc")
        self.ui = mercurial.ui.ui()
        self.ui.readconfig(hgrc, repo_path)
        mercurial.extensions.loadall(self.ui)
        self.repo = mercurial.hg.repository(self.ui, repo_path).unfiltered()

        self.in_file = sys.stdin
        self.out_file = sys.stdout

    def serve(self):
        while self.process_request():
            pass

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
        request_id, command, flags, data_len = header_fields

        # Read the request body
        body = self.in_file.read(data_len)
        if len(body) < data_len:
            raise Exception('received EOF after partial request')

        if command == CMD_MANIFEST:
            rev_name = body
            self.debug('sending manifest for revision %r', rev_name)
            self.dump_manifest(rev_name, request_id)
        else:
            logging.warning('unknown command %r', command)
            self.send_error(request_id, 'unknown command %r' % (command,))

        return True

    def send_chunk(self, request_id, data, is_last=True):
        flags = 0
        if not is_last:
            flags |= FLAG_MORE_CHUNKS
        self._send_chunk(request_id, command=CMD_RESPONSE,
                         flags=flags, data=data)

    def send_error(self, request_id, message):
        self._send_chunk(request_id, command=CMD_RESPONSE,
                         flags=FLAG_ERROR, data=message)

    def _send_chunk(self, request_id, command, flags, data):
        header = struct.pack(HEADER_FORMAT, request_id, command, flags,
                             len(data))
        self.out_file.write(header)
        self.out_file.write(data)
        self.out_file.flush()

    def dump_manifest(self, rev, request_id):
        '''
        Send the manifest data.
        '''
        start = time.time()
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
        for path in ctx:
            # Construct the chunk data using join(), since that is relatively
            # fast compared to other ways of constructing python strings.
            entry = b'\t'.join((mf[path], mf.flags(path), path + b'\0'))
            if len(chunked_paths) >= MANIFEST_PATHS_PER_CHUNK:
                num_paths += len(chunked_paths)
                self.send_chunk(request_id, b''.join(chunked_paths),
                                is_last=False)
                chunked_paths = [entry]
            else:
                chunked_paths.append(entry)

        num_paths += len(chunked_paths)
        self.send_chunk(request_id, b''.join(chunked_paths), is_last=True)
        self.debug('sent manifest with %d paths in %s seconds',
                   num_paths, time.time() - start)

    def prefetch(self, rev):
        if not hasattr(self.repo, 'prefetch'):
            # This repo isn't using remotefilelog, so nothing to do.
            return

        rev_range = mercurial.scmutil.revrange(self.repo, rev)
        self.debug('prefetching')
        self.repo.prefetch(rev_range)
        self.debug('done prefetching')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('repo', help='The repository path')
    args = parser.parse_args()

    logging.basicConfig(stream=sys.stderr, level=logging.DEBUG,
                        format='%(asctime)s %(message)s')
    HgServer(args.repo).serve()


if __name__ == '__main__':
    rc = main()
    sys.exit(rc)
