# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# Example classes to illustrate the usage of strict mock

from typing import Any


class StorageClient:
    def delete_for_real(self, path: str) -> bool:
        return False

    async def get_file_size(self, path: str) -> int:
        return 100


class StorageWithInjectedClient:
    def __init__(self, storage_client: Any) -> None:
        self.storage_client = storage_client

    def delete(self, path: str) -> bool:
        return self.storage_client.delete_for_real(path)


class StorageWithClientInConstructor:
    def __init__(self) -> None:
        self.storage = StorageClient()

    def delete(self, path: str) -> bool:
        return self.storage.delete_for_real(path)
