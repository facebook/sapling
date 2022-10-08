from abc import ABCMeta, abstractmethod
from typing import Any


class CircleCIEndpoint(metaclass=ABCMeta):
    async def get(self, path: str, **kwargs: Any) -> Any:
        """
        Send a GET request to endpoint 'path'.

        Returns: parsed JSON response
        """
        return await self.rest('get', path, **kwargs)

    async def post(self, path: str, **kwargs: Any) -> Any:
        """
        Send a POST request to endpoint 'path'.

        Returns: parsed JSON response
        """
        return await self.rest('post', path, **kwargs)

    @abstractmethod
    async def rest(self, method: str, path: str, **kwargs: Any) -> Any:
        """
        Send a 'method' request to endpoint 'path'.

        Args:
            method: 'GET', 'POST', etc.
            path: relative URL path to access on endpoint,
            does NOT include the API version number
            **kwargs: dictionary of JSON payload to send

        Returns: parsed JSON response
        """
        pass
