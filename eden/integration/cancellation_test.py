#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import asyncio

from eden.fs.service.eden.thrift_types import (
    CancelRequestsParams,
    DebugGetScmBlobRequest,
    EdenError,
    FaultDefinition,
    MountId,
    UnblockFaultArg,
)

from .lib import testcase


@testcase.eden_nfs_repo_test(run_coroutines=True)
class CancellationTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """Integration tests for the Thrift cancellation endpoint."""

    enable_fault_injection: bool = True

    def get_experimental_configs(self) -> list[str]:
        """Enable coroutines for debugGetBlob to support cancellation."""
        return ["enable-coroutines-debug-get-blob = true"]

    def populate_repo(self) -> None:
        self.repo.write_file("test_file.txt", "test content\n")
        self.repo.commit("Initial commit with test files")

    async def test_cancel_nonexistent_request(self) -> None:
        """Test cancelling a request that doesn't exist."""
        async with self.get_async_thrift_client() as client:
            params = CancelRequestsParams(requestIds=[99999])
            response = await client.cancelRequests(params)

            self.assertEqual(len(response.results), 1)
            result = response.results[0]
            self.assertIsNotNone(result.error)

    async def test_concurrent_cancel_requests(self) -> None:
        """Test making concurrent cancel requests."""
        async with self.get_async_thrift_client() as client:
            request_id = 54321

            tasks = []
            for _ in range(5):
                params = CancelRequestsParams(requestIds=[request_id])
                task = asyncio.create_task(client.cancelRequests(params))
                tasks.append(task)

            responses = await asyncio.gather(*tasks)

            for i, response in enumerate(responses):
                with self.subTest(request_index=i):
                    self.assertEqual(len(response.results), 1)
                    result = response.results[0]
                    self.assertIsNotNone(result.error)

    async def test_block_with_cancel_fault_injection(self) -> None:
        """Test cancelling a blockWithCancel fault injection, using debugGetBlob.

        7-step cancellation workflow:
        1. Inject blockWithCancel fault
        2. Make debugGetBlob thrift request (blocks with fault, polls for cancellation)
        3. Make getActiveRequests thrift request (shows active, uncancelled request with ID)
        4. Send cancel thrift request with the known request ID
        5. See that original debugGetBlob returns with cancellation message
        6. Make getActiveRequests thrift request
        7. See that the debugGetBlob request is no longer active
        """
        async with self.get_async_thrift_client() as client:
            fault = FaultDefinition(
                keyClass="debugGetBlob",
                keyValueRegex=".*",
                blockWithCancel=True,
                count=0,  # No expiration
            )
            await client.injectFault(fault)

            blob_request = DebugGetScmBlobRequest(
                mountId=MountId(mountPoint=self.mount_path_bytes),
                id=b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                origins=1,  # ANYWHERE
            )
            blob_task = asyncio.create_task(client.debugGetBlob(blob_request))

            await asyncio.sleep(0.1)

            debug_get_blob_request = None
            active_requests_response = await client.getActiveRequests()

            for req in active_requests_response.requests:
                if "debugGetBlob" in req.method:
                    debug_get_blob_request = req
                    break

            self.assertIsNotNone(
                debug_get_blob_request, "debugGetBlob request should be active"
            )
            self.assertTrue(
                debug_get_blob_request.cancelable,
                "debugGetBlob request should be cancelable",
            )

            request_id = debug_get_blob_request.requestId
            self.assertGreater(request_id, 0, "Request ID should be positive")

            cancel_params = CancelRequestsParams(requestIds=[request_id])
            cancel_response = await client.cancelRequests(cancel_params)

            self.assertEqual(len(cancel_response.results), 1)
            cancel_result = cancel_response.results[0]
            self.assertIsNotNone(cancel_result.success)
            self.assertEqual(cancel_result.success.requestId, request_id)

            await asyncio.sleep(0.1)

            with self.assertRaises(EdenError):
                await asyncio.wait_for(blob_task, timeout=5.0)

            final_active_requests = await client.getActiveRequests()

            cancelled_request_still_active = any(
                req.requestId == request_id for req in final_active_requests.requests
            )
            self.assertFalse(
                cancelled_request_still_active,
                f"Cancelled request {request_id} should no longer be active",
            )

            active_debug_get_blob_requests = [
                req
                for req in final_active_requests.requests
                if req.method == "debugGetBlob"
            ]
            self.assertEqual(
                len(active_debug_get_blob_requests),
                0,
                "No debugGetBlob requests should be active after cancellation",
            )

            unblock_info = UnblockFaultArg(keyClass="debugGetBlob", keyValueRegex=".*")
            await client.unblockFault(unblock_info)

    async def test_server_stop_cancels_requests(self) -> None:
        """Test that stopping the Eden server cancels blocked requests.

        Workflow:
        1. Inject blockWithCancel fault on debugGetBlob
        2. Make debugGetBlob thrift request (blocks with fault, polls for cancellation)
        3. Stop the Eden server
        4. See that original debugGetBlob returns with cancellation message (not timeout)
        """
        async with self.get_async_thrift_client() as client:
            fault = FaultDefinition(
                keyClass="debugGetBlob",
                keyValueRegex=".*",
                blockWithCancel=True,
                count=0,  # No expiration
            )
            await client.injectFault(fault)

            blob_request = DebugGetScmBlobRequest(
                mountId=MountId(mountPoint=self.mount_path_bytes),
                id=b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                origins=1,
            )
            blob_task = asyncio.create_task(client.debugGetBlob(blob_request))

            await asyncio.sleep(0.1)

            debug_get_blob_request = None
            active_requests_response = await client.getActiveRequests()

            for req in active_requests_response.requests:
                if "debugGetBlob" in req.method:
                    debug_get_blob_request = req
                    break

            self.assertIsNotNone(
                debug_get_blob_request, "debugGetBlob request should be active"
            )
            self.assertTrue(
                debug_get_blob_request.cancelable,
                "debugGetBlob request should be cancelable",
            )

            # Stop the Eden server - this should trigger cancellation
            self.eden.shutdown()

            with self.assertRaises(EdenError) as cm:
                await asyncio.wait_for(blob_task, timeout=5.0)

            error = cm.exception
            self.assertIn(
                "folly::OperationCancelled",
                error.message,
                "Expected folly::OperationCancelled in error message",
            )

    async def test_bulk_cancel_nonexistent_requests(self) -> None:
        """Test cancelling multiple non-existent requests in a single bulk operation."""
        async with self.get_async_thrift_client() as client:
            nonexistent_request_ids = [10001, 10002, 10003, 10004, 10005]
            params = CancelRequestsParams(requestIds=nonexistent_request_ids)
            response = await client.cancelRequests(params)

            self.assertEqual(len(response.results), 5)

            for i, result in enumerate(response.results):
                with self.subTest(request_index=i):
                    self.assertIsNotNone(result.error)

    async def test_simultaneous_cancel_multiple_requests(self) -> None:
        """Test injecting fault, starting 2 debugGetBlob calls, and cancelling both simultaneously."""
        async with self.get_async_thrift_client() as client:
            fault = FaultDefinition(
                keyClass="debugGetBlob",
                keyValueRegex=".*",
                blockWithCancel=True,
                count=0,  # No expiration
            )
            await client.injectFault(fault)

            request1 = DebugGetScmBlobRequest(
                mountId=MountId(mountPoint=self.mount_path_bytes),
                id=b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                origins=1,  # ANYWHERE
            )
            request2 = DebugGetScmBlobRequest(
                mountId=MountId(mountPoint=self.mount_path_bytes),
                id=b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                origins=1,  # ANYWHERE
            )

            task1 = asyncio.create_task(client.debugGetBlob(request1))
            task2 = asyncio.create_task(client.debugGetBlob(request2))

            await asyncio.sleep(0.1)

            active_requests_response = await client.getActiveRequests()
            debug_requests = [
                req
                for req in active_requests_response.requests
                if "debugGetBlob" in req.method
            ]

            self.assertEqual(
                len(debug_requests),
                2,
                "Should have exactly 2 active debugGetBlob requests",
            )

            request_id1 = debug_requests[0].requestId
            request_id2 = debug_requests[1].requestId

            cancel_params = CancelRequestsParams(requestIds=[request_id1, request_id2])
            cancel_response = await client.cancelRequests(cancel_params)

            self.assertEqual(len(cancel_response.results), 2)

            result1 = cancel_response.results[0]
            result2 = cancel_response.results[1]

            self.assertIsNotNone(result1.success)
            self.assertEqual(result1.success.requestId, request_id1)

            self.assertIsNotNone(result2.success)
            self.assertEqual(result2.success.requestId, request_id2)

            with self.assertRaises(EdenError):
                await asyncio.wait_for(task1, timeout=5.0)

            with self.assertRaises(EdenError):
                await asyncio.wait_for(task2, timeout=5.0)

            final_active_requests = await client.getActiveRequests()
            remaining_debug_requests = [
                req
                for req in final_active_requests.requests
                if "debugGetBlob" in req.method
            ]
            self.assertEqual(
                len(remaining_debug_requests),
                0,
                "No debugGetBlob requests should remain active after cancellation",
            )

            unblock_info = UnblockFaultArg(keyClass="debugGetBlob", keyValueRegex=".*")
            await client.unblockFault(unblock_info)
