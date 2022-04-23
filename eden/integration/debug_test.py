#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import os

from facebook.eden.ttypes import SyncBehavior

from .lib import testcase


@testcase.eden_repo_test
class DebugBlobTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("binary", b"\xff\xfe\xfd\xfc")
        self.repo.commit("Initial commit.")

    # TODO: enable when using the modern Python 3 Thrift API
    def xtest_debug_blob_prints_binary_data(self) -> None:
        with self.eden.get_thrift_client_legacy() as client:
            debugInfo = client.debugInodeStatus(
                os.fsencode(self.mount), b".", flags=0, sync=SyncBehavior()
            )

        [root] = [entry for entry in debugInfo if entry.path == b""]
        self.assertEqual(1, root.inodeNumber)

        [file] = [entry for entry in root.entries if entry.name == b"binary"]
        self.assertEqual(False, file.materialized)
        blob_id = binascii.hexlify(file.hash).decode()
        print(blob_id)

        output = self.eden.run_cmd("debug", "blob", ".", blob_id, cwd=self.mount)
        self.assertEqual(b"\xff\xfe\xfd\xfc", output)
