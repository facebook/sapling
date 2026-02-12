# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import math
from unittest import TestCase

import eden.mononoke.tests.mock_restricted_path.test_infra.python.frameworks_consolidation.storage as storage
from later.unittest import TestCase as LaterTestCase
from strictmock import (
    mock_async_callable,
    mock_callable,
    mock_constructor,
    patch_attribute,
    StrictMock,
)


# Uses unittest.TestCase
class UnittestStrictMockTest(TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.storage_client_mock = StrictMock(storage.StorageClient)

    def test_delete_from_storage(self) -> None:
        # Mock the delete_for_real method of the StorageClient class
        mock_callable(self.storage_client_mock, "delete_for_real").for_call(
            "file/to/delete"
        ).to_return_value(True).and_assert_called_once()

        # Call delete method of the StorageWrapper class
        storage.StorageWithInjectedClient(self.storage_client_mock).delete(
            "file/to/delete"
        )

    def test_with_mock_constructor(self) -> None:
        mock_constructor(
            "eden.mononoke.tests.mock_restricted_path.test_infra.python.frameworks_consolidation.storage",
            "StorageClient",
        ).for_call().to_return_value(self.storage_client_mock)

        mock_callable(self.storage_client_mock, "delete_for_real").for_call(
            "file/to/delete"
        ).to_return_value(True).and_assert_called_once()

        # Call delete method of the UsesStorage class
        storage.StorageWithClientInConstructor().delete("file/to/delete")

    def test_patch_pi(self) -> None:
        patch_attribute(math, "pi", 4)

        self.assertEqual(math.pi, 4)


# Uses later.unittest.TestCase
class LaterStrictMockTest(LaterTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.storage_client_mock = StrictMock(storage.StorageClient)

    def test_delete_from_storage(self) -> None:
        # Mock the delete_for_real method of the StorageClient class
        mock_callable(self.storage_client_mock, "delete_for_real").for_call(
            "file/to/delete"
        ).to_return_value(True).and_assert_called_once()

        # Call delete method of the StorageWrapper class
        storage.StorageWithInjectedClient(self.storage_client_mock).delete(
            "file/to/delete"
        )

    def test_with_mock_constructor(self) -> None:
        mock_constructor(
            "eden.mononoke.tests.mock_restricted_path.test_infra.python.frameworks_consolidation.storage",
            "StorageClient",
        ).for_call().to_return_value(self.storage_client_mock)

        mock_callable(self.storage_client_mock, "delete_for_real").for_call(
            "file/to/delete"
        ).to_return_value(True).and_assert_called_once()

        # Call delete method of the UsesStorage class
        storage.StorageWithClientInConstructor().delete("file/to/delete")

    def test_patch_pi(self) -> None:
        patch_attribute(math, "pi", 4)

        self.assertEqual(math.pi, 4)

    async def sample_async_func(self, _: str) -> int:
        return 43

    async def test_with_async(self) -> None:
        mock_async_callable(self.storage_client_mock, "get_file_size").for_call(
            "random/file"
        ).with_implementation(self.sample_async_func).and_assert_called_once()

        actual_file_size = await self.storage_client_mock.get_file_size("random/file")

        self.assertEqual(actual_file_size, 43)
