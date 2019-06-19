#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
This is an alternative version of hg_import_helper.py that is intended to intentionally
behave erroneously so we can test the error handling logic in the C++ HgImporter code.
"""

import argparse
import binascii
import errno
import json
import logging
import os
import struct
import sys

import hg_import_helper


class FakeRepo(object):
    pass


class FakeHgServer(hg_import_helper.HgServer):
    def __init__(self, repo_path, in_fd=None, out_fd=None):
        super(FakeHgServer, self).__init__(repo_path, {}, in_fd, out_fd)

    def initialize(self):
        # We don't actually open a real mercurial repository.
        # self.repo_path simply points to a directory with some test data for us to
        # load.
        self.repo = FakeRepo()

        data_path = os.path.join(self.repo_path, "data.json")
        with open(data_path, "r") as f:
            self.data = json.load(f)

    def _open_repo(self):
        raise Exception("should never be called")

    def _reopen_repo(self):
        raise Exception("should never be called")

    def _is_mononoke_supported(self, name):
        return name in ["fbsource"]

    def _gen_options(self):
        # We do not claim to support treemanifest, since treemanifest data
        # will be read directly by the C++ code rather than using our import helper
        # script.
        flags = 0
        treemanifest_paths = []

        # Options format:
        # - Protocol version number
        # - Is treemanifest supported?
        # - Number of treemanifest paths
        #   - treemanifest paths, encoded as (length, string_data)
        parts = []
        parts.append(
            struct.pack(
                b">III",
                hg_import_helper.PROTOCOL_VERSION,
                flags,
                len(treemanifest_paths),
            )
        )
        for path in treemanifest_paths:
            parts.append(struct.pack(b">I", len(path)))
            parts.append(path)

        return "".join(parts)

    def fetch_tree(self, path, manifest_node):
        # This should not be called since we disable treemanifest support
        raise Exception("TODO")

    def dump_manifest(self, rev, request):
        """
        Send the manifest data.
        """
        manifest = self.data["manifests"][rev]
        if self._do_error("manifest", rev):
            return

        results = []
        for path, flags, hashval in manifest:
            binhash = binascii.unhexlify(hashval)
            serialized_entry = b"\t".join((binhash, flags, path + b"\0"))
            results.append(serialized_entry)

        self.send_chunk(request, b"".join(results), is_last=True)

    def get_manifest_node(self, rev):
        # This should not be called since we disable treemanifest support
        raise Exception("TODO")

    def get_file(self, path, rev_hash):
        key = "%s:%s" % (path, binascii.hexlify(rev_hash))
        if self._do_error("blob", key):
            # Return an empty string.  The calling hg_import_helper.py code still
            # expects a response here.  It's okay if it sends an extra response after
            # the bogus one sent by _do_error()
            return ""
        return self.data["blobs"][key]

    def _do_prefetch(self, request):
        raise Exception("TODO")

    def _do_error(self, request_type, key):
        # Check if an error trigger file exists for this request
        key = key.replace(os.path.sep, "_")
        path = os.path.join(self.repo_path, "error.%s.%s" % (request_type, key))
        try:
            with open(path, "r") as f:
                error_type = f.read()
        except EnvironmentError as ex:
            if ex.errno == errno.ENOENT:
                return False
            raise

        if error_type.endswith("_once"):
            error_type = error_type[: -len("_once")]
            try:
                os.unlink(path)
            except EnvironmentError as ex:
                if ex.errno == errno.ENOENT:
                    # Maybe another import helper process got invoked and already
                    # performed this error action
                    return False
                raise

        if error_type == "exit":
            logging.error("triggering abnormal exit for test")
            os._exit(1)
        elif error_type == "bad_txn":
            txn_id = 12345678
            self._send_chunk(
                txn_id, command=hg_import_helper.CMD_RESPONSE, flags=0, data_blocks=[]
            )
            return True
        else:
            raise Exception("unknown error trigger type: %r" % (error_type,))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("repo", help="The repository path")
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

    args = parser.parse_args()
    logging.basicConfig(
        stream=sys.stderr, level=logging.INFO, format="%(asctime)s %(message)s"
    )

    server = FakeHgServer(args.repo, in_fd=args.in_fd, out_fd=args.out_fd)
    try:
        return server.serve()
    except KeyboardInterrupt:
        logging.debug("hg_import_helper received interrupt; shutting down")


if __name__ == "__main__":
    rc = main()
    sys.exit(rc)
