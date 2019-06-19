#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import contextlib
import logging
import os
import pathlib
import re
import subprocess
import types
import typing


pystemd_import_error = None
try:
    import pystemd
    import pystemd.dbusexc  # pyre-ignore[21]: T32805591
    import pystemd.dbuslib  # pyre-ignore[21]: T32805591
    import pystemd.systemd1.manager
    import pystemd.systemd1.unit
except ModuleNotFoundError as e:
    pystemd_import_error = e


logger = logging.getLogger(__name__)


_T = typing.TypeVar("_T")


def edenfs_systemd_service_name(eden_dir: pathlib.Path) -> str:
    assert isinstance(eden_dir, pathlib.PosixPath)
    instance_name = systemd_escape_path(eden_dir)
    return f"fb-edenfs@{instance_name}.service"


def systemd_escape_path(path: pathlib.PurePosixPath) -> str:
    """Escape a path for inclusion in a systemd unit name.

    See the 'systemd-escape --path' command for details.
    """
    if not path.is_absolute():
        raise ValueError("systemd_escape_path can only escape absolute paths")
    if ".." in path.parts:
        raise ValueError(
            "systemd_escape_path can only escape paths without '..' components"
        )
    stdout: bytes = subprocess.check_output(
        ["systemd-escape", "--path", "--", str(path)]
    )
    return stdout.decode("utf-8").rstrip("\n")


class EdenFSSystemdServiceConfig:
    __eden_dir: pathlib.Path
    __edenfs_executable_path: pathlib.Path
    __extra_edenfs_arguments: typing.List[str]

    def __init__(
        self,
        eden_dir: pathlib.Path,
        edenfs_executable_path: pathlib.Path,
        extra_edenfs_arguments: typing.Sequence[str],
    ) -> None:
        super().__init__()
        self.__eden_dir = eden_dir
        self.__edenfs_executable_path = edenfs_executable_path
        self.__extra_edenfs_arguments = list(extra_edenfs_arguments)

    @property
    def config_file_path(self) -> pathlib.Path:
        return self.__eden_dir / "systemd.conf"

    @property
    def startup_log_file_path(self) -> pathlib.Path:
        # TODO(T33122320): Move this into <eden_dir>/logs/.
        return self.__eden_dir / "startup.log"

    def write_config_file(self) -> None:
        variables = {
            b"EDENFS_EXECUTABLE_PATH": bytes(self.__edenfs_executable_path),
            b"EDENFS_EXTRA_ARGUMENTS": self.__escape_argument_list(
                self.__extra_edenfs_arguments
            ),
        }
        self.config_file_path.parent.mkdir(parents=True, exist_ok=True)
        self.config_file_path.write_bytes(SystemdEnvironmentFile.dumps(variables))

    @staticmethod
    def __escape_argument_list(arguments: typing.Sequence[str]) -> bytes:
        for argument in arguments:
            if "\n" in arguments:
                raise ValueError(
                    f"Newlines in arguments are not supported\nArgument: {argument!r}"
                )
        return b"\n".join(arg.encode("utf-8") for arg in arguments)


class SystemdEnvironmentFile:
    _comment_characters = b"#;"
    _escape_characters = b"\\"
    _name_characters = (
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_"
    )
    _newline_characters = b"\n\r"
    _quote_characters = b"'\""
    _whitespace_characters = b" \t"

    def __init__(self, entries: typing.Sequence[typing.Tuple[bytes, bytes]]) -> None:
        super().__init__()
        self.__entries = list(entries)

    @classmethod
    def loads(cls, content: bytes) -> "SystemdEnvironmentFile":
        content = _truncated_at_null_byte(content)
        entries = _EnvironmentFileParser(content).parse_entries()
        return cls(entries=entries)

    @classmethod
    def dumps(cls, variables: typing.Mapping[bytes, bytes]) -> bytes:
        output = bytearray()
        for name, value in variables.items():
            cls._validate_entry(name, value)
            output.extend(name)
            output.extend(b"=")
            output.extend(cls.__escape_value(value))
            output.extend(b"\n")
        return bytes(output)

    @staticmethod
    def __escape_value(value: bytes) -> bytes:
        return (
            b'"'
            + re.sub(b'[\\\\"]', lambda match: b"\\" + match.group(0), value)
            + b'"'
        )

    @classmethod
    def _is_valid_entry(cls, name: bytes, value: bytes) -> bool:
        try:
            cls._validate_entry(name, value)
            return True
        except (VariableNameError, VariableValueError):
            return False

    @classmethod
    def _validate_entry(cls, name: bytes, value: bytes) -> None:
        if not name:
            raise VariableNameError("Variables must have a non-empty name")
        if name[0:1].isdigit():
            raise VariableNameError("Variable names must not begin with a digit")
        for c in name:
            if c in cls._whitespace_characters:
                raise VariableNameError("Variable names must not contain whitespace")
            if c in cls._newline_characters:
                raise VariableNameError(
                    "Variable names must not contain any newline characters"
                )
            if c < 0x20:
                raise VariableNameError(
                    f"Variable names must not contain any control characters"
                )
            if c < 0x80 and c not in cls._name_characters:
                offending_character = bytes([c]).decode("utf-8")
                raise VariableNameError(
                    f"Variable names must not contain '{offending_character}'"
                )
        for c in value:
            if c in b"\r":
                raise VariableValueError(
                    "Variable values must not contain carriage returns"
                )
            if c < 0x20 and c not in b"\n\t":
                raise VariableValueError(
                    "Variable values must not contain any control characters"
                )

    @property
    def entries(self) -> typing.List[typing.Tuple[bytes, bytes]]:
        return self.__entries


class VariableNameError(ValueError):
    pass


class VariableValueError(ValueError):
    pass


class _Scanner:
    def __init__(self, input: bytes) -> None:
        super().__init__()
        self.__input = input
        self.__index = 0

    @property
    def at_eof(self) -> bool:
        return self.__index == len(self.__input)

    def scan_one_byte(self) -> int:
        if self.at_eof:
            raise ValueError("Cannot scan past end of file")
        c = self.__input[self.__index]
        self.__index += 1
        return c

    def peek_one_byte(self) -> int:
        if self.at_eof:
            raise ValueError("Cannot peek past end of file")
        return self.__input[self.__index]

    def skip_one_byte(self) -> None:
        if self.at_eof:
            raise ValueError("Cannot skip past end of file")
        self.__index += 1

    def scan_while_any(self, scan_bytes: typing.Sequence[int]) -> bytes:
        return self.__scan_while(lambda c: c in scan_bytes)

    def scan_until_any(self, stop_bytes: typing.Sequence[int]) -> bytes:
        return self.__scan_while(lambda c: c not in stop_bytes)

    def skip_while_any(self, skip_bytes: typing.Sequence[int]) -> None:
        self.__skip_while(lambda c: c in skip_bytes)

    def skip_until_any(self, stop_bytes: typing.Sequence[int]) -> None:
        self.__skip_while(lambda c: c not in stop_bytes)

    def __scan_while(self, scan_predicate: typing.Callable[[int], bool]) -> bytes:
        begin_index = self.__index
        while not self.at_eof:
            if not scan_predicate(self.__input[self.__index]):
                break
            self.__index += 1
        end_index = self.__index
        return self.__input[begin_index:end_index]

    def __skip_while(self, skip_predicate: typing.Callable[[int], bool]) -> None:
        while not self.at_eof:
            if not skip_predicate(self.__input[self.__index]):
                break
            self.__index += 1


class _EnvironmentFileParser(_Scanner):
    comment_characters = SystemdEnvironmentFile._comment_characters
    escape_characters = SystemdEnvironmentFile._escape_characters
    newline_characters = SystemdEnvironmentFile._newline_characters
    quote_characters = SystemdEnvironmentFile._quote_characters
    whitespace_characters = SystemdEnvironmentFile._whitespace_characters

    def parse_entries(self) -> typing.List[typing.Tuple[bytes, bytes]]:
        entries = []
        while not self.at_eof:
            entry = self.parse_entry()
            if entry is not None:
                entries.append(entry)
        return entries

    def parse_entry(self) -> typing.Optional[typing.Tuple[bytes, bytes]]:
        self.skip_whitespace()
        if self.at_eof:
            return None
        c = self.peek_one_byte()
        if c in self.comment_characters:
            self.parse_comment()
            return None
        elif c in self.newline_characters:
            self.skip_one_byte()
            return None

        name = self.parse_entry_name_and_equal_sign()
        if name is None:
            return None
        self.skip_whitespace()
        value = self.parse_entry_value()
        if not SystemdEnvironmentFile._is_valid_entry(name, value):
            return None
        return (name, value)

    def parse_entry_name_and_equal_sign(self) -> typing.Optional[bytes]:
        name = bytearray([self.scan_one_byte()])
        name.extend(self.scan_until_any(b"=" + self.newline_characters))
        if self.at_eof:
            return None
        c = self.scan_one_byte()
        if c in self.newline_characters:
            return None
        assert c == b"="[0]
        return bytes(name.rstrip(self.whitespace_characters))

    def parse_entry_value(self) -> bytes:
        value = bytearray()
        self.parse_quoted_entry_value(out_value=value)
        self.parse_unquoted_entry_value(out_value=value)
        return bytes(value)

    def parse_quoted_entry_value(self, out_value: bytearray) -> None:
        while not self.at_eof:
            c = self.peek_one_byte()
            if c not in self.quote_characters:
                return
            terminating_quote_characters = bytes([c])

            self.skip_one_byte()

            while not self.at_eof:
                scanned = self.scan_until_any(
                    self.escape_characters + terminating_quote_characters
                )
                out_value.extend(scanned)
                if self.at_eof:
                    return

                c = self.scan_one_byte()
                if c in self.escape_characters:
                    if self.at_eof:
                        return
                    c = self.scan_one_byte()
                    if c not in self.newline_characters:
                        out_value.append(c)
                elif c in terminating_quote_characters:
                    break
                else:
                    raise AssertionError()

            self.skip_whitespace()

    def parse_unquoted_entry_value(self, out_value: bytearray) -> None:
        while not self.at_eof:
            scanned = self.scan_until_any(
                self.escape_characters
                + self.newline_characters
                + self.whitespace_characters
            )
            out_value.extend(scanned)
            if self.at_eof:
                return

            c = self.scan_one_byte()
            if c in self.escape_characters:
                if self.at_eof:
                    return
                c = self.scan_one_byte()
                if c not in self.newline_characters:
                    out_value.append(c)
            elif c in self.newline_characters:
                return
            elif c in self.whitespace_characters:
                scanned = self.scan_while_any(self.whitespace_characters)
                is_trailing_whitespace = (
                    self.at_eof or self.peek_one_byte() in self.newline_characters
                )
                if is_trailing_whitespace:
                    return
                out_value.append(c)
                out_value.extend(scanned)
            else:
                raise AssertionError()

    def parse_comment(self) -> None:
        c = self.scan_one_byte()
        assert c in self.comment_characters
        while not self.at_eof:
            self.skip_until_any(self.escape_characters + self.newline_characters)
            if self.at_eof:
                break
            c = self.scan_one_byte()
            if c in self.escape_characters:
                if self.at_eof:
                    break
                self.skip_one_byte()
            elif c in self.newline_characters:
                break
            else:
                raise AssertionError()

    def skip_whitespace(self) -> None:
        self.skip_while_any(self.whitespace_characters)


def _truncated_at_null_byte(data: bytes) -> bytes:
    end_of_file_index = data.find(b"\x00")
    if end_of_file_index == -1:
        return data
    return data[:end_of_file_index]


async def print_service_status_using_systemctl_for_diagnostics_async(
    service_name: str, xdg_runtime_dir: str
) -> None:
    systemctl_environment = dict(os.environ)
    systemctl_environment["XDG_RUNTIME_DIR"] = xdg_runtime_dir
    status_process = await asyncio.create_subprocess_exec(
        "systemctl",
        "--no-pager",
        "--user",
        "status",
        "--",
        service_name,
        env=systemctl_environment,
    )
    _status_exit_code = await status_process.wait()
    # Ignore the status exit code. We only run systemctl for improved
    # diagnostics.
    _status_exit_code


# Types of parameters for D-Bus methods and signals. For details, see D-Bus'
# documentation:
# https://dbus.freedesktop.org/doc/dbus-specification.html#type-system
DBusObjectPath = bytes
DBusString = bytes
DBusUint32 = int


class SystemdUserBus:
    """A communication channel with systemd.

    See systemd's D-Bus documentation:
    https://www.freedesktop.org/wiki/Software/systemd/dbus/
    """

    _cleanups: contextlib.ExitStack
    _dbus: "pystemd.dbuslib.DBus"
    _event_loop: asyncio.AbstractEventLoop
    _manager: "pystemd.SDManager"

    def __init__(
        self, event_loop: asyncio.AbstractEventLoop, xdg_runtime_dir: str
    ) -> None:
        if pystemd_import_error is not None:
            raise pystemd_import_error

        super().__init__()
        self._cleanups = contextlib.ExitStack()
        self._dbus = self._get_dbus(xdg_runtime_dir)
        self._event_loop = event_loop
        self._manager = pystemd.systemd1.manager.Manager(bus=self._dbus)

    @staticmethod
    def _get_dbus(
        xdg_runtime_dir: str,
    ) -> "pystemd.dbuslib.DBus":  # pyre-ignore[11]: T32805591
        # HACK(strager): pystemd.dbuslib.DBus(user_mode=True) fails with a
        # connection timeout. 'SYSTEMCTL_FORCE_BUS=1 systemctl --user ...' also
        # fails, and it seems to use the same C APIs as
        # pystemd.dbuslib.DBus(user_mode=True). Work around the issue by doing
        # what systemctl's internal bus_connect_user_systemd() function does
        # [1].
        #
        # [1] https://github.com/systemd/systemd/blob/78a562ee4bcbc7b0e8b58b475ff656f646e95e40/src/shared/bus-util.c#L594
        socket_path = pathlib.Path(xdg_runtime_dir) / "systemd" / "private"
        return pystemd.dbuslib.DBusAddress(  # pyre-ignore[16]: T32805591
            b"unix:path=" + escape_dbus_address(bytes(socket_path))
        )

    def open(self) -> None:
        self._cleanups.enter_context(self._dbus)
        self._manager.load()
        self._add_to_event_loop()

    def close(self) -> None:
        self._cleanups.close()

    def _add_to_event_loop(self) -> None:
        dbus_fd = self._dbus.get_fd()
        self._event_loop.add_reader(dbus_fd, self._process_queued_messages)
        self._cleanups.callback(lambda: self._event_loop.remove_reader(dbus_fd))

    def _process_queued_messages(self) -> None:
        while True:
            message = self._dbus.process()
            if message.is_empty():
                break

    async def get_unit_active_state_async(self, unit_name: bytes) -> DBusString:
        """Query org.freedesktop.systemd1.Unit.ActiveState.
        """

        def go() -> DBusString:
            unit = pystemd.systemd1.unit.Unit(unit_name, bus=self._dbus)
            unit.load()
            active_state = _pystemd_dynamic(unit).Unit.ActiveState
            assert isinstance(active_state, DBusString)
            return active_state

        return await self._run_in_executor_async(go)

    async def get_service_result_async(self, service_name: bytes) -> DBusString:
        """Query org.freedesktop.systemd1.Service.Result.
        """

        def go() -> DBusString:
            unit = pystemd.systemd1.unit.Unit(service_name, bus=self._dbus)
            unit.load()
            result = _pystemd_dynamic(unit).Service.Result
            assert isinstance(result, DBusString)
            return result

        return await self._run_in_executor_async(go)

    async def start_service_and_wait_async(self, service_name: DBusString) -> None:
        """Start a service, waiting for it to successfully start.

        If the service or the job fails, this method raises an exception.
        """
        start_job = await self.start_unit_job_and_wait_until_job_completes_async(
            service_name
        )
        logger.debug(f"Querying status of service {service_name!r}...")
        (service_active_state, service_result) = await asyncio.gather(
            self.get_unit_active_state_async(unit_name=service_name),
            self.get_service_result_async(service_name=service_name),
        )
        logger.debug(
            f"Service {service_name!r} has active state "
            f"{service_active_state!r} and result {service_result!r}"
        )
        if not (
            start_job.result == b"done"
            and service_active_state == b"active"
            and service_result == b"success"
        ):
            raise SystemdServiceFailedToStartError(
                service_name=service_name.decode(errors="replace"),
                start_job_result=start_job.result.decode(errors="replace"),
                service_active_state=service_active_state.decode(errors="replace"),
                service_result=service_result.decode(errors="replace"),
            )

    async def start_unit_job_and_wait_until_job_completes_async(
        self, unit_name: DBusString
    ) -> "JobRemovedSignal":
        """Call org.freedesktop.systemd1.Manager.StartUnit and wait for the
        returned job to complete.

        If the job fails, this method does *not* raise an exception.
        """
        with await self.subscribe_to_job_removed_async() as job_removed_subscription:
            logger.debug(f"Starting service {unit_name!r}...")
            job_object_path = await self.start_unit_async(
                name=unit_name, mode=b"replace"
            )
            logger.debug(f"Waiting for job {job_object_path!r} to finish...")
            removed_job = await job_removed_subscription.wait_until_signal_async(
                lambda removed_job: removed_job.job == job_object_path
            )
            logger.debug(
                f"Job {job_object_path!r} for {unit_name!r} finished "
                f"with result {removed_job.result!r}"
            )
            return removed_job

    async def start_unit_async(
        self, name: DBusString, mode: DBusString
    ) -> DBusObjectPath:
        """Call org.freedesktop.systemd1.Manager.StartUnit.
        """

        def go() -> DBusObjectPath:
            path = _pystemd_dynamic(self._manager).Manager.StartUnit(name, mode)
            assert isinstance(path, DBusObjectPath)
            return path

        return await self._run_in_executor_async(go)

    async def subscribe_to_job_removed_async(
        self
    ) -> "SystemdSignalSubscription[JobRemovedSignal]":
        """Subscribe to org.freedesktop.systemd1.Manager.JobRemoved.
        """
        subscription: SystemdSignalSubscription[
            JobRemovedSignal
        ] = SystemdSignalSubscription(self._manager)
        await asyncio.gather(
            self._subscribe_async(),
            self._run_in_executor_async(
                lambda: self._dbus.match_signal(
                    sender=b"org.freedesktop.systemd1",
                    path=b"/org/freedesktop/systemd1",
                    interface=b"org.freedesktop.systemd1.Manager",
                    member=b"JobRemoved",
                    callback=self._on_job_removed,
                    userdata=(subscription, self._event_loop),
                )
            ),
        )
        return subscription

    @staticmethod
    def _on_job_removed(
        msg: "pystemd.dbuslib.DbusMessage",  # pyre-ignore[11]: T32805591
        error: typing.Optional[Exception],
        userdata: typing.Any,
    ) -> None:
        """Handle a org.freedesktop.systemd1.Manager.JobRemoved signal.
        """
        (subscription, event_loop) = userdata
        assert isinstance(subscription, DBusSignalSubscription)
        assert isinstance(event_loop, asyncio.AbstractEventLoop)

        if error is not None:
            event_loop.create_task(subscription.post_exception_async(error))
            return

        try:
            msg.process_reply(False)
            (id, job, unit, result) = msg.body
            event_loop.create_task(
                subscription.post_signal_async(
                    JobRemovedSignal(id=id, job=job, unit=unit, result=result)
                )
            )
        except Exception as e:
            event_loop.create_task(subscription.post_exception_async(e))

    async def _subscribe_async(self) -> None:
        """Call org.freedesktop.systemd1.Manager.Subscribe.
        """

        def go() -> None:
            _pystemd_dynamic(self._manager).Manager.Subscribe()

        await self._run_in_executor_async(go)

    async def _run_in_executor_async(self, func: typing.Callable[[], "_T"]) -> "_T":
        return await self._event_loop.run_in_executor(executor=None, func=func)

    def __enter__(self):
        self.open()
        return self

    def __exit__(
        self,
        exc_type: typing.Optional[typing.Type[BaseException]],
        exc_value: typing.Optional[BaseException],
        traceback: typing.Optional[types.TracebackType],
    ) -> None:
        self.close()


_DBusSignal = typing.TypeVar("_DBusSignal")


class DBusSignalSubscription(typing.Generic[_DBusSignal]):
    _queue: "asyncio.Queue[typing.Union[_DBusSignal, Exception]]"

    def __init__(self) -> None:
        super().__init__()
        self._queue = asyncio.Queue()

    async def post_signal_async(self, signal: _DBusSignal) -> None:
        await self._queue.put(signal)

    async def post_exception_async(self, exception: Exception) -> None:
        await self._queue.put(exception)

    async def get_next_signal_async(self) -> _DBusSignal:
        signal_or_exception = await self._queue.get()
        if isinstance(signal_or_exception, Exception):
            raise signal_or_exception
        return signal_or_exception

    async def wait_until_signal_async(
        self, predicate: typing.Callable[[_DBusSignal], bool]
    ) -> _DBusSignal:
        while True:
            signal = await self.get_next_signal_async()
            if predicate(signal):
                return signal

    def unsubscribe(self) -> None:
        # TODO(strager): Add an API in pystemd to cancel a match_signal request.
        logger.debug("Leaking D-Bus signal subscription")

    def __enter__(self):
        return self

    def __exit__(
        self,
        exc_type: typing.Optional[typing.Type[BaseException]],
        exc_value: typing.Optional[BaseException],
        traceback: typing.Optional[types.TracebackType],
    ) -> None:
        self.unsubscribe()


class SystemdSignalSubscription(DBusSignalSubscription[_DBusSignal]):
    _manager: "pystemd.SDManager"

    def __init__(self, manager: "pystemd.SDManager") -> None:
        super().__init__()
        self._manager = manager

    def unsubscribe(self) -> None:
        super().unsubscribe()
        _pystemd_dynamic(self._manager).Manager.Unsubscribe()


class JobRemovedSignal(typing.NamedTuple):
    """A org.freedesktop.systemd1.Manager.JobRemoved signal.

    https://www.freedesktop.org/wiki/Software/systemd/dbus/#signals
    """

    id: DBusUint32
    job: DBusObjectPath
    unit: DBusString
    result: DBusString


def _pystemd_dynamic(
    object: typing.Union[
        "pystemd.systemd1.manager.Manager", "pystemd.systemd1.unit.Unit"
    ]
) -> typing.Any:
    """Silence mypy and Pyre for the given dynamically-typed pystemd object.

    TODO(strager): Add type annotations to pystemd.
    """
    return typing.cast(typing.Any, object)


def escape_dbus_address(input: bytes) -> bytes:
    """Escape a string for inclusion in DBUS_SESSION_BUS_ADDRESS.

    For more details, see the D-Bus specification:
    https://dbus.freedesktop.org/doc/dbus-specification.html#addresses
    """
    whitelist = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-./\\"

    scanner = _Scanner(input)
    result_pieces = []
    while not scanner.at_eof:
        unescaped_bytes = scanner.scan_while_any(whitelist)
        result_pieces.append(unescaped_bytes)
        if scanner.at_eof:
            break
        byte_to_escape = scanner.scan_one_byte()
        result_pieces.append(f"%{byte_to_escape:02x}".encode())
    return b"".join(result_pieces)


if pystemd_import_error is None:
    SystemdConnectionRefusedError = (
        pystemd.dbusexc.DBusConnectionRefusedError  # pyre-ignore[16]: T32805591
    )
    SystemdFileNotFoundError = (
        pystemd.dbusexc.DBusFileNotFoundError  # pyre-ignore[16]: T32805591
    )
else:
    SystemdConnectionRefusedError = Exception
    SystemdFileNotFoundError = Exception


class SystemdServiceFailedToStartError(Exception):
    def __init__(
        self,
        start_job_result: str,
        service_active_state: str,
        service_name: str,
        service_result: str,
    ) -> None:
        super().__init__()
        self.service_active_state = service_active_state
        self.service_name = service_name
        self.service_result = service_result
        self.start_job_result = start_job_result

    def __str__(self) -> str:
        return (
            f"Starting the {self.service_name} systemd service failed "
            f"(reason: {self.service_result})"
        )
