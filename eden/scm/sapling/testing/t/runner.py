# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import doctest
import json
import os
import queue
import re
import subprocess
import sys
import tempfile
import textwrap
import threading
import traceback
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable, List, Optional, Union

from . import hghave
from .runtime import hasfeature, Mismatch
from .transform import transform


@dataclass
class TestId:
    """information about a test identity"""

    name: str
    path: str

    @classmethod
    def frompath(cls, path: str):
        if path.startswith("doctest:"):
            name = path
            modname = name[8:]
            __import__(modname)
            # pyre-fixme[9]: path has type `str`; used as `Optional[str]`.
            path = sys.modules[modname].__file__
            return cls(name=name, path=path)

        path = os.path.abspath(path)
        if path.endswith(".py"):
            # try to convert the .py path to doctest:module
            modnames = sorted(n for n in sys.modules if "." not in n)
            for name in modnames:
                mod = sys.modules[name]
                modpath = getattr(mod, "__file__", None)
                if not modpath or os.path.basename(modpath) != "__init__.py":
                    continue
                prefix = os.path.dirname(modpath) + os.path.sep
                if not path.startswith(prefix):
                    continue
                relpath = path[len(prefix) :].replace("\\", "/")
                for suffix in ["/__init__.py", ".py"]:
                    if relpath.endswith(suffix):
                        relpath = relpath[: -len(suffix)]
                        break
                modname = f"{mod.__name__}.{relpath.replace('/', '.')}"
                return cls.frompath(f"doctest:{modname}")
            # try harder, using sys.path
            # This is needed when the modules being used are static:*.
            for root in sys.path:
                relpath = os.path.relpath(path, root)
                if not relpath.startswith(".."):
                    if relpath.endswith("__init__.py"):
                        # strip "/__init__.py"
                        name = os.path.dirname(relpath)
                    else:
                        # strip ".py"
                        name = relpath[:-3]
                    modname = name.replace("\\", ".").replace("/", ".")
                    if modname in sys.modules:
                        # double check that the source code matches
                        mod = sys.modules[modname]

                        if mod.__file__.startswith("static:"):
                            import inspect

                            source1 = inspect.getsource(mod)
                            with open(path, "rb") as f:
                                source2 = f.read().decode()

                            if source1 != source2:
                                sys.stderr.write(
                                    f"warning: doctest is using an older static version of module {modname} that no longer matches on-disk {path}\n"
                                )
                                sys.stderr.flush()

                        return cls.frompath(f"doctest:{modname}")
            raise RuntimeError(
                f"cannot find Python module name for {path=} to run doctest\n"
                "hint: try 'doctest:module.name' instead of file path\n"
            )
        else:
            name = os.path.basename(path)
            return cls(name=name, path=path)

    @property
    def modname(self) -> Optional[str]:
        if self.name.startswith("doctest:"):
            return self.name.split(":", 1)[1]
        return None


@dataclass
class TestResult:
    testid: TestId
    exc_type: Optional[str] = None
    exc_msg: Optional[str] = None
    tb: Optional[str] = None


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

    def __init__(
        self, paths: List[str], jobs: int = 1, exts=List[str], isolate: bool = True
    ):
        self.testids = [TestId.frompath(p) for p in paths]
        self.jobs = jobs or os.cpu_count()
        self.exts = exts
        self.isolate = isolate

    def __iter__(self):
        return self

    def __next__(self) -> Union[TestResult, Mismatch]:
        """obtain next test result, or mismatch block"""
        try:
            # pyre-fixme[16]: `TestRunner` has no attribute `resultqueue`.
            v = self.resultqueue.get()
            if v is StopIteration:
                raise StopIteration
            return v
        except ValueError:
            raise StopIteration

    def __enter__(self):
        """prepare the test runner environment, namely a way to receive test results"""
        self.tempdir = tempfile.TemporaryDirectory(prefix="sl-testing-ipc")
        self.resultqueue = queue.Queue()
        self.sem = threading.Semaphore(self.jobs)
        self.running = True
        if self.isolate:
            # start running tests in background.
            # This thread will be "tailing" the resultqueue - see "__next__".
            self.runnerthread = thread = threading.Thread(
                target=self._start, daemon=True
            )
            thread.start()
        else:
            self._start()
        return self

    def __exit__(self, et, ev, tb):
        """stop and clean up test environment"""
        self.running = False
        self.tempdir.cleanup()
        if self.isolate:
            self.runnerthread.join()

    def _start(self):
        """run tests (blocking, intended to run in thread)"""
        processes = []
        threads = []
        try:
            for t in self.testids:
                # child process will write to this file to report progress
                output_path = os.path.join(self.tempdir.name, f"{t.name}-ipc")
                with open(output_path, "w"):
                    pass
                kwargs = {
                    "testid": t,
                    "output_path": output_path,
                    "exts": self.exts,
                }
                if self.isolate:
                    # limit concurrency, release by the Process
                    while True:
                        acquired = self.sem.acquire(timeout=1)
                        if not self.running:
                            return
                        if acquired:
                            break
                    p = _spawn_runtest(**kwargs)
                    processes.append(p)
                    t = threading.Thread(
                        target=self._tail_child, args=(p, output_path), daemon=True
                    )
                    t.start()
                    threads.append(t)
                else:
                    runtest_reporting_progress(**kwargs)
                    self._tail_child(None, output_path)
        finally:
            for p in processes:
                p.wait()
            for t in threads:
                t.join()
            self.resultqueue.put(StopIteration)

    def _tail_child(self, child, output_path):
        """monitor output from child, push to resultqueue"""
        try:
            with open(output_path, "r") as f:
                while True:
                    line = f.readline()
                    if not line:
                        child_alive = child and child.poll() is None
                        if child_alive:
                            # child might still write to this file.
                            try:
                                child.wait(0.5)
                            except subprocess.TimeoutExpired:
                                pass
                            continue
                        else:
                            # EOF. child can no longer write to this file.
                            break
                    obj_type, obj_data = json.loads(line)
                    if obj_type == "Mismatch":
                        obj = Mismatch(**obj_data)
                    elif obj_type == "TestResult":
                        obj_data["testid"] = TestId(**obj_data["testid"])
                        obj = TestResult(**obj_data)
                    else:
                        raise TypeError(f"Unexpected type name: {obj_type}")
                    self.resultqueue.put(obj)
        finally:
            self.sem.release()


def _spawn_runtest(
    testid: TestId, exts: List[str], output_path: str
) -> subprocess.Popen:
    args = [sys.executable]
    if "sapling.commands" in sys.modules:
        args.append("debugpython")
    args += ["-m", "sapling.testing.single"]
    args += [f"--ext={ext}" for ext in exts]
    args += [f"--structured-output={output_path}", "--no-default-exts", testid.path]
    return subprocess.Popen(args, shell=False)


def runtest_reporting_progress(
    testid: TestId,
    exts: List[str],
    output_path: str,
):
    """runtest, report progress as JSON objects to output_path"""

    hasmismatch = False

    def write_progress(obj):
        with open(output_path, "a") as f:
            f.write(json.dumps([type(obj).__name__, asdict(obj)]) + "\n")

    def mismatchcb(mismatch: Mismatch):
        nonlocal hasmismatch
        hasmismatch = True
        mismatch.testname = testid.name
        write_progress(mismatch)

    result = TestResult(testid=testid)
    try:
        runtest(testid, exts, mismatchcb)
    except Exception as e:
        result.exc_type = type(e).__name__
        result.exc_msg = str(e)
        if not isinstance(e, TestNotFoundError):
            result.tb = traceback.format_exc(limit=-1)
    finally:
        if result.exc_type is None and hasmismatch:
            result.exc_type = "MismatchError"
            result.exc_msg = "output mismatch"
        write_progress(result)


class TestNotFoundError(FileNotFoundError):
    def __str__(self):
        return "not found"


def runtest(testid: TestId, exts: List[str], mismatchcb: Callable[[Mismatch], None]):
    """run a .t test at the given path

    The generated Python code is written at __pycache__/ttest/<test>.py.
    Return output mismatches.
    """
    if testid.modname:
        return rundoctest(testid, mismatchcb)
    else:
        return runttest(testid, exts, mismatchcb)


class doctestrunner(doctest.DocTestRunner):
    """doctest runner that reports output mismatches as Mismatch"""

    def __init__(self, testname: str, mismatchcb: Callable[[Mismatch], None]):
        optionflags = doctest.IGNORE_EXCEPTION_DETAIL
        super().__init__(verbose=False, optionflags=optionflags)
        self.testname = testname
        self.mismatchcb = mismatchcb

    def report_failure(
        self, out, test: doctest.DocTest, example: doctest.Example, got: str
    ):
        # see doctest.OutputChecker.output_difference
        if not (self.optionflags & doctest.DONT_ACCEPT_BLANKLINE):
            got = re.sub("(?m)^[ ]*(?=\n)", doctest.BLANKLINE_MARKER, got)

        # pyre-fixme[58]: `+` is not supported for operand types `Optional[int]` and
        #  `int`.
        srcloc = test.lineno + example.lineno
        outloc = srcloc + example.source.count("\n")
        endloc = outloc + example.want.count("\n")
        src = ">>> " + textwrap.indent(example.source, "... ")[4:]
        mismatch = Mismatch(
            actual=got,
            expected=example.want,
            src=src,
            srcloc=srcloc,
            outloc=outloc,
            endloc=endloc,
            indent=example.indent,
            # pyre-fixme[6]: For 8th param expected `str` but got `Optional[str]`.
            filename=test.filename,
            testname=self.testname,
        )
        self.mismatchcb(mismatch)

    def report_unexpected_exception(self, out, test, example, excinfo):
        exctype, excvalue, exctb = excinfo
        excmsg = str(excvalue)
        exctypestr = exctype.__name__
        if excmsg:
            excstr = f"{exctypestr}: {excmsg}"
        else:
            excstr = exctypestr
        got = f"Traceback (most recent call last):\n  ...\n{excstr}\n"
        return self.report_failure(out, test, example, got)


def rundoctest(testid: TestId, mismatchcb: Callable[[Mismatch], None]):
    """run doctest for the given module, report Mismatch via mismatchcb"""
    modname = testid.modname
    # pyre-fixme[6]: For 1st param expected `str` but got `Optional[str]`.
    __import__(modname)
    # pyre-fixme[6]: For 1st param expected `str` but got `Optional[str]`.
    mod = sys.modules[modname]
    finder = doctest.DocTestFinder()
    runner = doctestrunner(testid.name, mismatchcb)
    for test in finder.find(mod):
        runner.run(test)


def runttest(testid: TestId, exts: List[str], mismatchcb: Callable[[Mismatch], None]):
    path = Path(testid.path)
    testdir = path.parent
    exts = exts[:]
    try:
        tcode = path.read_bytes().decode()
    except FileNotFoundError as e:
        raise TestNotFoundError(e)

    exeneeded = set()

    def hasfeaturetracked(feature):
        if feature in {"ext.parallel"}:
            exts.append("sapling.testing.ext.parallel")
            return True
        result = hasfeature(feature)
        if result and feature in hghave.exes:
            exeneeded.add(feature)
        return result

    testcases = []

    body = transform(
        tcode,
        indent=8,
        filename=str(path),
        hasfeature=hasfeaturetracked,
        registertestcase=testcases.append,
    )

    testcases = [f"_run_once(testcase={repr(tc)})\n" for tc in testcases]

    if not testcases:
        testcases.append("_run_once()\n")

    extcode = []
    for ext in exts:
        extcode.append(
            f"""
__import__({repr(ext)})
sys.modules[{repr(ext)}].testsetup(t)
"""
        )

    extcode = textwrap.indent("".join(extcode).strip(), "    ")

    pycode = f"""
import sys
from sapling.testing.t.runtime import TestTmp

TESTFILE = {repr(str(path))}
TESTDIR = {repr(str(testdir))}

def _run_once(testcase=None):
    t = TestTmp(tmpprefix={repr(path.name)}, testcase=testcase)
    t.setenv("TESTFILE", TESTFILE)
    t.setenv("TESTDIR", TESTDIR)
    t.setenv("DEBUGRUNTEST_ENABLED", "1")
    t.setenv("RUNTESTDIR", TESTDIR)  # compatibility: path of run-tests.py

    for exe in {sorted(exeneeded)}:
        t.requireexe(exe)

    sys.path += [TESTDIR, str(t.path)]

{extcode}

    with t:
        _pristine_globals = dict(globals())
        globals().update(t.pyenv)

{body}

        # Restore globals since pydoceval locals are persisted as
        # global variables (and we don't want variables crossing test
        # cases).
        globals().clear()
        globals().update(_pristine_globals)

{"".join(testcases)}
"""

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
