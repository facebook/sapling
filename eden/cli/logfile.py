#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import contextlib
import io
import pathlib
import typing


_BinaryIO = typing.Union[typing.IO[bytes], io.BufferedIOBase]


@contextlib.contextmanager
def forward_log_file(
    file_path: pathlib.Path, output_file: _BinaryIO
) -> typing.Iterator["LogForwarder"]:
    with follow_log_file(file_path) as follower:
        yield LogForwarder(follower=follower, output_file=output_file)


class LogForwarder:
    __follower: "LogFollower"
    __output_file: _BinaryIO

    def __init__(self, follower: "LogFollower", output_file: _BinaryIO) -> None:
        super().__init__()
        self.__follower = follower
        self.__output_file = output_file

    def poll(self) -> None:
        # pyre-fixme[29]: `Union[Callable[[Union[bytearray, bytes]], int],
        #  Callable[[bytes], int]]` is not a function.
        self.__output_file.write(self.__follower.poll())
        self.__output_file.flush()

    async def poll_forever_async(self) -> typing.NoReturn:
        while True:
            self.poll()
            await asyncio.sleep(0.1)


@contextlib.contextmanager
def follow_log_file(file_path: pathlib.Path) -> typing.Iterator["LogFollower"]:
    with open(file_path, "rb") as file:
        yield LogFollower(file)


class LogFollower:
    __file: _BinaryIO

    def __init__(self, file: _BinaryIO) -> None:
        super().__init__()
        self.__file = file

    def poll(self) -> bytes:
        # pyre-fixme[29]: `Union[Callable[[Optional[int]], bytes], Callable[[int],
        #  bytes]]` is not a function.
        return self.__file.read()
