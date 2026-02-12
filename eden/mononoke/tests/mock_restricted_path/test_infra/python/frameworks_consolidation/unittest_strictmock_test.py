# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict


# Example classes to illustrate the usage of strict mock
class StorageClient:
    def delete_for_real(self, path: str) -> bool:
        return False


class StorageWrapper:
    def __init__(self, storage_client: StorageClient) -> None:
        self.storage_client = storage_client

    def delete(self, path: str) -> bool:
        return self.storage_client.delete_for_real(path)


# Actual tests
from testslide import StrictMock, TestCase


# Uses testslide.TestCase instead of unittest.TestCase
class UnittestStrictmockTest(TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.storage_client_mock = StrictMock(StorageClient)

    def test_delete_from_storage(self) -> None:
        # Mock the delete_for_real method of the StorageClient class
        self.mock_callable(self.storage_client_mock, "delete_for_real").for_call(
            "file/to/delete"
        ).to_return_value(True).and_assert_called_once()

        # Call delete method of the StorageWrapper class
        StorageWrapper(self.storage_client_mock).delete("file/to/delete")
