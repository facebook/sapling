#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import os
import sys
from pathlib import Path

from eden.thrift.legacy import EdenClient

from facebook.eden.ttypes import (
    DataFetchOrigin,
    DebugGetScmBlobRequest,
    DebugGetScmBlobResponse,
    MountId,
    ScmBlobOrError,
    ScmBlobWithOrigin,
    SyncBehavior,
)

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


@testcase.eden_nfs_repo_test
class DebugBlobHgTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("binary", b"\xff\xfe\xfd\xfc")
        self.repo.commit("Initial commit.")

    def assert_blob_not_available(
        self, client: EdenClient, blob_id: bytes, origin: int
    ) -> None:
        response = client.debugGetBlob(
            DebugGetScmBlobRequest(
                MountId(self.mount.encode()),
                blob_id,
                origin,
            )
        )
        print(response)
        # this should not error
        response.blobs[0].blob.get_error(),

    def assert_blob_available(
        self,
        client: EdenClient,
        blob_id: bytes,
        origin: DataFetchOrigin,
        data: bytes,
    ) -> None:
        response = client.debugGetBlob(
            DebugGetScmBlobRequest(
                MountId(self.mount.encode()),
                blob_id,
                origin,
            )
        )
        print(response)
        self.assertEqual(
            DebugGetScmBlobResponse(
                [ScmBlobWithOrigin(blob=ScmBlobOrError(blob=data), origin=origin)]
            ),
            response,
        )

    def test_debug_blob_locations(self) -> None:
        with self.eden.get_thrift_client_legacy() as client:
            debugInfo = client.debugInodeStatus(
                os.fsencode(self.mount), b".", flags=0, sync=SyncBehavior()
            )
        [root] = [entry for entry in debugInfo if entry.path == b""]
        self.assertEqual(1, root.inodeNumber)

        [file] = [entry for entry in root.entries if entry.name == b"binary"]
        self.assertEqual(False, file.materialized)
        blob_id = binascii.hexlify(file.hash).decode()
        print(file.hash)
        print(blob_id)

        self.eden.run_cmd("gc", cwd=self.mount)

        with self.eden.get_thrift_client_legacy() as client:
            # not present in the local storage yet
            for origin in [
                DataFetchOrigin.MEMORY_CACHE,
                DataFetchOrigin.DISK_CACHE,
            ]:
                print(origin)
                print(file.hash)
                self.assert_blob_not_available(
                    client,
                    file.hash,
                    origin,
                )

            # "fetch from network"
            self.assert_blob_available(
                client,
                file.hash,
                DataFetchOrigin.ANYWHERE,
                b"\xff\xfe\xfd\xfc",
            )

            # now its available locally.
            for fromWhere in [
                DataFetchOrigin.DISK_CACHE,
                DataFetchOrigin.LOCAL_BACKING_STORE,
                # reading a blob is actually insuffient to put it in
                # DataFetchFromWhere.MEMORY_CACHE,
            ]:
                print(fromWhere)
                self.assert_blob_available(
                    client, file.hash, fromWhere, b"\xff\xfe\xfd\xfc"
                )

            # check a request from multiple places:
            response = client.debugGetBlob(
                DebugGetScmBlobRequest(
                    MountId(self.mount.encode()),
                    file.hash,
                    DataFetchOrigin.MEMORY_CACHE
                    | DataFetchOrigin.DISK_CACHE
                    | DataFetchOrigin.LOCAL_BACKING_STORE
                    | DataFetchOrigin.REMOTE_BACKING_STORE
                    | DataFetchOrigin.ANYWHERE,
                )
            )
            print(response)

            self.assertEqual(5, len(response.blobs))
            for blob in response.blobs:
                if blob.origin == DataFetchOrigin.MEMORY_CACHE:
                    blob.blob.get_error()
                elif blob.origin == DataFetchOrigin.DISK_CACHE:
                    self.assertEqual(b"\xff\xfe\xfd\xfc", blob.blob.get_blob())
                elif blob.origin == DataFetchOrigin.LOCAL_BACKING_STORE:
                    self.assertEqual(b"\xff\xfe\xfd\xfc", blob.blob.get_blob())
                elif blob.origin == DataFetchOrigin.REMOTE_BACKING_STORE:
                    blob.blob.get_error()
                elif blob.origin == DataFetchOrigin.ANYWHERE:
                    self.assertEqual(b"\xff\xfe\xfd\xfc", blob.blob.get_blob())

            if sys.platform != "win32":
                # on non windows platforms materializing an inode does cache it's
                # original blob contents, so now it should be in the local store.
                with open(Path(self.mount) / "binary", "a") as binary_file:
                    binary_file.buffer.write(b"\xfc")

                self.assert_blob_available(
                    client,
                    file.hash,
                    DataFetchOrigin.MEMORY_CACHE,
                    b"\xff\xfe\xfd\xfc",
                )
