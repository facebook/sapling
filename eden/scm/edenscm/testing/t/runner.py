# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import multiprocessing
import os
import sys
import textwrap
import threading
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Set, Callable, Optional, Iterable, Union

from .runtime import TestTmp, Mismatch, hasfeature
from .transform import transform


@dataclass
class TestId:
    """infomration about a test identity"""

    name: str
    path: str

    @classmethod
    def frompath(cls, path: str):
        path = os.path.abspath(path)
        name = os.path.basename(path)
        return cls(name=name, path=path)


@dataclass
class TestResult:
    testid: TestId
    exc: Optional[Exception] = None
    tb: Optional[str] = None


class MismatchError(AssertionError):
    pass


class TestRunner:
    """runner for .t tests

    The runner reports test progress in a streaming fashion and does
    not write to stdout or stderr.

    With context is used for explicit resource cleanup:

        mismatches = []
        with TestRunner(paths) as runner:
            for item in runner:
                if isinstance(item, Mismatch):
                    # print mismatch
                    mismatches.append(item)
                    ...
                else:
                    assert isinstance(item, TestResult)
                    # do something with TestResult
        if autofix:
            fixmismatches(mismatches)
    """

    def __init__(self, paths: List[str], jobs: int = 1, exts=List[str]):
        self.testids = [TestId.frompath(p) for p in paths]
        self.jobs = jobs or multiprocessing.cpu_count()
        self.exts = exts

    def __iter__(self):
        return self

    def __next__(self) -> Union[TestResult, Mismatch]:
        """obtain next test result, or mismatch block"""
        try:
            v = self.resultqueue.get()
            if v is StopIteration:
                raise StopIteration
            return v
        except ValueError:
            raise StopIteration

    def __enter__(self):
        """prepare the test runner environment, namely a way to receive test results"""
        # use 'spawn' instead of 'fork' to clean up Python global state.
        # Python global state might be polluted by uisetup or tests.
        self.mp = mp = multiprocessing.get_context("spawn")
        self.resultqueue = mp.Queue()
        self.sem = mp.Semaphore(self.jobs)
        # start running tests in background
        self.runnerthread = thread = threading.Thread(target=self._start, daemon=True)
        self.running = True
        thread.start()
        return self

    def __exit__(self, et, ev, tb):
        """stop and clean up test environment"""
        self.running = False
        self.runnerthread.join()

    def _start(self):
        """run tests (blocking, intended to run in thread)"""
        processes = []
        try:
            for t in self.testids:
                # limit concurrency, release by the Process
                while True:
                    acquired = self.sem.acquire(timeout=1)
                    if not self.running:
                        return
                    if acquired:
                        break
                p = self.mp.Process(
                    target=_spawnmain,
                    kwargs={
                        "testid": t,
                        "sem": self.sem,
                        "resultqueue": self.resultqueue,
                        "exts": self.exts,
                    },
                )
                p.start()
                processes.append(p)
        finally:
            for p in processes:
                p.join()
            self.resultqueue.put(StopIteration)


def _spawnmain(
    testid: TestId,
    exts: List[str],
    sem: multiprocessing.Semaphore,
    resultqueue: multiprocessing.Queue,
):
    """run a test and report progress back
    intended to be spawned via multiprocessing.Process.
    """

    hasmismatch = False

    def mismatchcb(mismatch: Mismatch):
        nonlocal hasmismatch
        hasmismatch = True
        mismatch.testname = testid.name
        resultqueue.put(mismatch)

    result = TestResult(testid=testid)
    try:
        runtest(testid, exts, mismatchcb)
    except TestNotFoundError as e:
        result.exc = e
    except Exception as e:
        result.exc = e
        result.tb = traceback.format_exc(limit=-1)
    finally:
        if result.exc is None and hasmismatch:
            result.exc = MismatchError("output mismatch")
        resultqueue.put(result)
        resultqueue.close()
        sem.release()


class TestNotFoundError(FileNotFoundError):
    def __str__(self):
        return "not found"


def runtest(testid: TestId, exts: List[str], mismatchcb: Callable[[Mismatch], None]):
    """run a .t test at the given path

    The generated Python code is written at __pycache__/ttest/<test>.py.
    Return output mismatches.
    """
    path = Path(testid.path)
    testdir = path.parent


    try:
        tcode = path.read_bytes().decode()
    except FileNotFoundError as e:
        raise TestNotFoundError(e)

    body = transform(tcode, indent=4, filename=str(path), hasfeature=hasfeature)

    extcode = []
    for ext in exts:
        extcode.append(
            f"""
__import__({repr(ext)})
sys.modules[{repr(ext)}].testsetup(t)
"""
        )
    header = f"""
import sys
from edenscm.testing.t.runtime import TestTmp

TESTFILE = {repr(str(path))}
TESTDIR = {repr(str(testdir))}

t = TestTmp(tmpprefix={repr(path.name)})
t.setenv("TESTFILE", TESTFILE)
t.setenv("TESTDIR", TESTDIR)
t.setenv("RUNTESTDIR", TESTDIR)  # compatibility: path of run-tests.py

sys.path += [TESTDIR, str(t.path)]

{"".join(extcode)}

with t:
    globals().update(t.pyenv)
"""

    pycode = header + body
    pypath = (path.parent / "__pycache__" / "ttest" / path.name).with_suffix(".py")

    # write it down for easier investigation
    pypath.parent.mkdir(parents=True, exist_ok=True)
    pypath.write_bytes(pycode.encode())

    compiled = compile(pycode, str(pypath), "exec")
    pyenv = {"mismatchcb": mismatchcb}
    exec(compiled, pyenv)


class filelinesdict(collections.defaultdict):
    """{path: [line]} dict - read path on demand"""

    def __missing__(self, key: str) -> List[str]:
        with open(key, "rb") as f:
            lines = f.read().decode().splitlines(True)
            self[key] = lines
        return lines


def fixmismatches(mismatches: List[Mismatch]):
    """update test files to fix mismatches"""
    mismatches = sorted(mismatches, key=lambda m: (m.filename, -m.outloc))
    filelines = filelinesdict()
    lastline = collections.defaultdict(lambda: sys.maxsize)
    for m in mismatches:
        if lastline[m.filename] < m.outloc:
            # already changed or fixed (ex. conflicted fix in a loop)
            continue
        lastline[m.filename] = m.outloc
        lines = filelines[m.filename]
        # TODO: try to preserve (glob)s.
        # 'lambda l: True' ensures blank lines are indented too
        lines[m.outloc : m.endloc] = textwrap.indent(
            m.actual, " " * m.indent, lambda l: True
        )
    for path, lines in filelines.items():
        with open(path, "wb") as f:
            f.write("".join(lines).encode())
