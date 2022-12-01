# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from abc import abstractmethod
from typing import Generic, NoReturn, Optional, TypeVar

T = TypeVar("T")
E = TypeVar("E")


class Result(Generic[T, E]):
    """
    A minimal implementation of Rust like Result type.
    https://doc.rust-lang.org/std/result/enum.Result.html
    """

    @abstractmethod
    def is_ok(self) -> bool:
        pass

    @abstractmethod
    def is_err(self) -> bool:
        pass

    @abstractmethod
    def ok(self) -> Optional[T]:
        pass

    @abstractmethod
    def err(self) -> Optional[E]:
        pass

    @abstractmethod
    def unwrap(self) -> T:
        pass

    @abstractmethod
    def unwrap_err(self) -> E:
        pass


class Ok(Generic[T, E], Result[T, E]):
    """
    Contains the success value

    >>> v = Ok(1)
    >>> v
    Ok(1)
    >>> v.is_ok()
    True
    >>> v.is_err()
    False
    >>> v.ok()
    1
    >>> v.err()
    >>> v.unwrap()
    1
    >>> v.unwrap_err()
    Traceback (most recent call last):
      ...
    edenscm.result.UnwrapError: called `Result.unwrap_err()` on an `Ok` value Ok(1)
    """

    def __init__(self, value: T) -> None:
        self._value = value

    def __repr__(self) -> str:
        return f"Ok({self._value!r})"

    def is_ok(self) -> bool:
        return True

    def is_err(self) -> bool:
        return False

    def ok(self) -> T:
        return self._value

    def err(self) -> None:
        return None

    def unwrap(self) -> T:
        return self._value

    def unwrap_err(self) -> NoReturn:
        raise UnwrapError(f"called `Result.unwrap_err()` on an `Ok` value {self!r}")


class Err(Generic[T, E], Result[T, E]):
    """
    Contains the failure value

    >>> v = Err(1)
    >>> v
    Err(1)
    >>> v.is_ok()
    False
    >>> v.is_err()
    True
    >>> v.ok()
    >>> v.err()
    1
    >>> v.unwrap()
    Traceback (most recent call last):
      ...
    edenscm.result.UnwrapError: called `Result.unwrap()` on an `Err` value Err(1)
    >>> v.unwrap_err()
    1
    """

    def __init__(self, value: E) -> None:
        self._value = value

    def __repr__(self) -> str:
        return f"Err({self._value!r})"

    def is_ok(self) -> bool:
        return False

    def is_err(self) -> bool:
        return True

    def ok(self) -> None:
        return None

    def err(self) -> E:
        return self._value

    def unwrap(self) -> NoReturn:
        raise UnwrapError(f"called `Result.unwrap()` on an `Err` value {self!r}")

    def unwrap_err(self) -> E:
        return self._value


class UnwrapError(Exception):
    """
    Exception that indicates something has gone wrong in an unwrap call.
    """

    pass
