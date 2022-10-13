import contextlib
import datetime
import functools
import logging
import os
import re
import shutil
import subprocess
import sys
import uuid
from typing import Dict, Iterator, Optional

DATETIME_FORMAT = '%Y-%m-%d_%Hh%Mm%Ss'


RE_LOG_DIRNAME = re.compile(
    r'(\d{4}-\d\d-\d\d_\d\dh\d\dm\d\ds)_'
    r'[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}')


class Formatter(logging.Formatter):
    redactions: Dict[str, str]

    def __init__(self, fmt: Optional[str] = None,
                 datefmt: Optional[str] = None):
        super().__init__(fmt, datefmt)
        self.redactions = {}

    # Remove sensitive information from URLs
    def _filter(self, s: str) -> str:
        s = re.sub(r':\/\/(.*?)\@', r'://<USERNAME>:<PASSWORD>@', s)
        for needle, replace in self.redactions.items():
            s = s.replace(needle, replace)
        return s

    def formatMessage(self, record: logging.LogRecord) -> str:
        if record.levelno == logging.INFO or record.levelno == logging.DEBUG:
            # Log INFO/DEBUG without any adornment
            return record.getMessage()
        else:
            # I'm not sure why, but formatMessage doesn't show up
            # even though it's in the typeshed for Python >3
            return super().formatMessage(record)  # type: ignore

    def format(self, record: logging.LogRecord) -> str:
        return self._filter(super().format(record))

    # Redact specific strings; e.g., authorization tokens.  This won't
    # retroactively redact stuff you've already leaked, so make sure
    # you redact things as soon as possible
    def redact(self, needle: str, replace: str = '<REDACTED>') -> None:
        # Don't redact empty strings; this will lead to something
        # that looks like s<REDACTED>t<REDACTED>r<REDACTED>...
        if needle == '':
            return
        self.redactions[needle] = replace


formatter = Formatter(
    fmt="%(levelname)s: %(message)s", datefmt="")


@contextlib.contextmanager
def manager(*, debug: bool = False) -> Iterator[None]:
    # TCB code to setup logging.  If a failure starts here we won't
    # be able to save the user in a reasonable way.

    setup(stderr_level=logging.DEBUG if debug else logging.INFO,
          file_level=logging.DEBUG)

    record_argv()

    try:
        # Do logging rotation
        rotate()

        yield

    except Exception as e:
        logging.exception("Fatal exception")
        record_exception(e)
        sys.exit(1)

    except BaseException as e:
        # You could logging.debug here to suppress the backtrace
        # entirely, but there is no reason to hide it from technically
        # savvy users.
        logging.info("", exc_info=True)
        record_exception(e)
        sys.exit(1)

_sapling_cli = "sl"

def setup(stderr_level: int = logging.WARN,
          file_level: int = logging.DEBUG,
          sapling_cli: str = _sapling_cli):

    global _sapling_cli
    _sapling_cli = sapling_cli

    # Logging structure: there is one logger (the root logger) and in
    # processes all events. There are two handlers: stderr and file
    # handler.
    root_logger = logging.getLogger()
    root_logger.setLevel(logging.DEBUG)

    console_handler = logging.StreamHandler()
    console_handler.setLevel(stderr_level)
    console_handler.setFormatter(formatter)
    root_logger.addHandler(console_handler)

    log_file = os.path.join(run_dir(), "ghstack.log")

    file_handler = logging.FileHandler(log_file)
    file_handler.setLevel(file_level)
    # TODO: Hypothetically, it is better if we log the timestamp.
    # But I personally feel the timestamps gunk up the log info
    # for not much benefit (since we're not really going to be
    # in the business of debugging performance bugs, for which
    # timestamps would really be helpful.)  Perhaps reconsider
    # at some point based on how useful this information actually is.
    #
    # If you ever switch this, make sure to preserve redaction
    # logic...
    file_handler.setFormatter(formatter)
    # file_handler.setFormatter(logging.Formatter(
    #    fmt="[%(asctime)s] [%(levelname)8s] %(message)s"))
    root_logger.addHandler(file_handler)


@functools.lru_cache()
def base_dir() -> str:
    # Don't use shell here as we are not allowed to log yet!
    try:
        meta_dir = subprocess.run(
            ("git", "rev-parse", "--git-dir"),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            encoding='utf-8'
        ).stdout.rstrip()
    except subprocess.CalledProcessError:
        meta_dir = subprocess.run(
            (_sapling_cli, "root", "--dotdir"), stdout=subprocess.PIPE,
            encoding='utf-8',
            check=True
        ).stdout.rstrip()

    base_dir = os.path.join(meta_dir, "ghstack", "log")

    try:
        os.makedirs(base_dir)
    except FileExistsError:
        pass

    return base_dir


# Naughty, "run it once and save" memoizing
@functools.lru_cache()
def run_dir() -> str:
    # NB: respects timezone
    cur_dir = os.path.join(
        base_dir(),
        "{}_{}"
        .format(datetime.datetime.now().strftime(DATETIME_FORMAT),
                uuid.uuid1()))

    try:
        os.makedirs(cur_dir)
    except FileExistsError:
        pass

    return cur_dir


def record_exception(e: BaseException) -> None:
    with open(os.path.join(run_dir(), "exception"), 'w') as f:
        f.write(type(e).__name__)


@functools.lru_cache()
def record_argv() -> None:
    with open(os.path.join(run_dir(), "argv"), 'w') as f:
        f.write(subprocess.list2cmdline(sys.argv))


def record_status(status: str) -> None:
    with open(os.path.join(run_dir(), "status"), 'w') as f:
        f.write(status)


def rotate() -> None:
    log_base = base_dir()
    old_logs = os.listdir(log_base)
    old_logs.sort(reverse=True)
    for stale_log in old_logs[1000:]:
        # Sanity check that it looks like a log
        assert RE_LOG_DIRNAME.fullmatch(stale_log)
        shutil.rmtree(os.path.join(log_base, stale_log))
