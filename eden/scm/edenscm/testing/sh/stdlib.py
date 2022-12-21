# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""standard library for shell

A subset of "standard" coreutils or shell builtin commands.
For .t test specific commands such as "hg", look at t/runtime.py
instead.
"""

import sys
import tarfile
from functools import wraps
from io import BytesIO
from typing import BinaryIO, Callable, Dict, Iterator, List, Optional, Tuple

from .types import Env, InterpResult, Scope, ShellExit, ShellFS, ShellReturn

cmdtable = {}
SKIP_PYTHON_LOOKUP = True


def command(commandfunc):
    """decorator to register a shell command implemented in Python

    Registered commands can be used as builtin commands in shinterp.

    The commandfunc takes arguments like args, arg0, env, stdin,
    stdout, stderr, fs. They will be provided from 'env: Env'.

    The output could be in 2 forms:

        (str): conveniently write to stdout
        (int): specify exit code
        (None): same as 0
    """
    wrapper = wrap(commandfunc)
    cmdtable[commandfunc.__name__] = wrapper
    return commandfunc


def wrap(commandfunc) -> Callable[[Env], InterpResult]:
    co = commandfunc.__code__
    coargs = set(co.co_varnames[: co.co_argcount])

    @wraps(commandfunc)
    def wrapper(env: Env) -> InterpResult:
        kwargs = {}
        if "env" in coargs:
            kwargs["env"] = env
        if "args" in coargs:
            kwargs["args"] = env.args[1:]
        if "arg0" in coargs:
            kwargs["arg0"] = env.args[0]
        if "fs" in coargs:
            kwargs["fs"] = env.fs
        # tempoarily allocated BytesIO on demand
        allocated: Dict[str, BytesIO] = {}
        for name in ["stdin", "stdout", "stderr"]:
            if name in coargs:
                f = getattr(env, name, None)
                if f is None:
                    if name == "stderr" and "stdout" in allocated:
                        # mix stderr and stdout in one stream if both need to
                        # be allocated
                        f = allocated["stdout"]
                    else:
                        f = BytesIO()
                        allocated[name] = f
                kwargs[name] = f
        ret = commandfunc(**kwargs)
        exitcode = 0
        out = ""
        for name in ["stdout", "stderr"]:
            f = allocated.get(name)
            if f is not None:
                out += f.getvalue().decode(errors="replace")
        if ret is None:
            pass
        elif isinstance(ret, int):
            exitcode = ret
        elif isinstance(ret, str):
            if env.stdout is None:
                out += ret
            else:
                env.stdout.write(ret.encode())
        else:
            raise TypeError(
                f"callable {commandfunc} returned {ret} but expect int or str"
            )
        return InterpResult(
            out=out,
            exitcode=exitcode,
        )

    return wrapper


@command
def echo(args: List[str]) -> str:
    eol = "\n"
    if args and args[0].startswith("-") and not args[0].startswith("--"):
        flags, *args = args
        for flag in flags[1:]:
            if flag == "n":
                eol = ""
            else:
                raise NotImplementedError(f"echo {flags}")
    return " ".join(args) + eol


@command
def printf(args: List[str]):
    fmt = (
        args[0]
        .replace(r"\n", "\n")
        .replace(r"\0", "\0")
        .replace(r"\r", "\r")
        .replace(r"\t", "\t")
    )
    needed = fmt.count("%") - fmt.count("%%") * 2
    i = 1
    out = []
    if not needed:
        out.append(fmt)
    else:
        while i < len(args):
            fmtargs = args[i : i + needed]
            if len(fmtargs) < needed:
                fmtargs += ["" * (needed - len(args))]
            i += needed
            out.append(fmt % tuple(fmtargs))
    return "".join(out)


@command
def env(args: List[str], env: Env) -> str:
    if args:
        raise NotImplementedError("env with args")
    out = "".join(f"{k}={v}\n" for k, v in env.getexportedenv().items())
    return out


@command
def export(args: List[str], env: Env):
    # affect the shared "exportedenvvars"
    # pyre-fixme[9]: env has type `Env`; used as `Optional[Env]`.
    env = env.parent
    for arg in args:
        if "=" in arg:
            name, value = arg.split("=", 1)
            env.setenv(name, value, Scope.SHELL)
        else:
            name = arg
        env.exportenv(name)


@command
def unset(args: List[str], env: Env):
    # pyre-fixme[9]: env has type `Env`; used as `Optional[Env]`.
    env = env.parent
    for name in args:
        env.unset(name, Scope.SHELL)


@command
def local(args: List[str], env: Env):
    for arg in args:
        if "=" not in arg:
            arg += "="
        name, value = arg.split("=", 1)
        env.localenv(name)
        env.setenv(name, value, Scope.FUNCTION)


@command
def wait(args: List[str], stdout: BinaryIO, env: Env):
    if args:
        raise NotImplementedError(f"wait {args=}")
    for (thread, jobout) in env.jobs:
        thread.join()
        stdout.write(jobout.getvalue())
    env.jobs.clear()
    return 0


@command
def true():
    pass


@command
def false():
    return 1


@command
def cat(
    args: List[str], stdout: BinaryIO, stdin: BinaryIO, stderr: BinaryIO, fs: ShellFS
) -> int:
    exitcode = 0

    def reporterror(path, message):
        nonlocal exitcode
        exitcode = 1
        stderr.write(f"cat: {path}: {message}\n".encode())

    lines = list(_lines(fs, args, stdin, reporterror=reporterror))
    stdout.write(b"".join(lines))
    return exitcode


@command
def dos2unix(
    args: List[str],
    stdout: BinaryIO,
    stdin: BinaryIO,
    fs: ShellFS,
):
    text = b"".join(_lines(fs, args, stdin)).replace(b"\r\n", b"\n")
    stdout.write(text)


@command
def tee(
    args: List[str], stdout: BinaryIO, stderr: BinaryIO, stdin: BinaryIO, fs: ShellFS
):
    data = stdin.read()
    for path in args:
        with fs.open(path, "wb") as f:
            f.write(data)
    stdout.write(data)


@command
def touch(args: List[str], fs: ShellFS):
    if args[0] == "-t":
        utimestr, *args = args[1:]
        # YYYYMMDDhhmm
        if len(utimestr) != 12 or "." in utimestr:
            raise NotImplementedError(f"touch -t {utimestr}")

        import datetime

        u = utimestr
        d = datetime.datetime(
            int(u[:4]),
            int(u[4:6]),
            int(u[6:8]),
            int(u[8:10]),
            int(u[10:12]),
            tzinfo=datetime.timezone.utc,
        )

        utime = int(d.timestamp())
    else:
        utime = None
    for path in args:
        with fs.open(path, "ab"):
            pass
        if utime is not None:
            fs.utime(path, utime)


@command
def test(args: List[str], arg0: str, env: Env):
    neg = False
    if args and args[0] == "!":
        neg = True
        args = args[1:]
    if (arg0, args[-1]) in (("[", "]"), ("[[", "]]")):
        args = args[:-1]
    istrue: Optional[bool] = None
    if len(args) == 3:
        op = args[1]
        if op in {"-gt", "-lt", "-ge", "-le", "-eq", "-ne"}:
            lhs = int(args[0] or "0")
            rhs = int(args[2] or "0")
            istrue = getattr(lhs, f"__{op[1:]}__")(rhs)
        if op in {"=", "==", "!="}:
            lhs = args[0]
            rhs = args[2]
            istrue = lhs == rhs
            if op == "!=":
                istrue = not istrue
    elif len(args) == 2:
        op, arg = args
        fs = env.fs
        if op == "-n":
            istrue = bool(arg)
        elif op == "-z":
            istrue = not bool(arg)
        elif op == "-f":
            istrue = fs.isfile(arg)
        elif op == "-d":
            istrue = fs.isdir(arg)
        elif op == "-e":
            try:
                fs.stat(arg)
                istrue = True
            except FileNotFoundError:
                istrue = False
        elif op == "-x":
            import stat

            # pyre-fixme[9]: istrue has type `Optional[bool]`; used as `int`.
            istrue = fs.stat(arg).st_mode & stat.S_IEXEC
    if istrue is None:
        raise NotImplementedError(f"test {args} is not implemented")
    if neg:
        istrue = not istrue
    return int(not istrue)


@command
def head(args: List[str], stdin: BinaryIO, stdout: BinaryIO, fs: ShellFS):
    n, paths = _parseheadtail(args)
    lines = list(_lines(fs, paths, stdin))
    stdout.write(b"".join(lines[:n]))


@command
def tail(args: List[str], stdin: BinaryIO, stdout: BinaryIO, fs: ShellFS):
    n, paths = _parseheadtail(args)
    lines = list(_lines(fs, paths, stdin))
    stdout.write(b"".join(lines[-n:]))


@command
def seq(args: List[str]) -> str:
    start = 1
    step = 1
    end = int(args[-1])
    if len(args) == 1:
        start = 1
    elif len(args) == 2:
        start = int(args[0])
    elif len(args) == 3:
        step = int(args[1])
        assert step > 0
    values = range(start, end + 1, step)
    return "".join(f"{i}\n" for i in values)


@command
def sed(args: List[str], stdin: BinaryIO, stdout: BinaryIO, fs: ShellFS) -> str:
    scripts = []
    paths = []
    inplace = False
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "-e":
            i += 1
            scripts.append(args[i])
        elif arg == "-i":
            inplace = True
        elif not scripts:
            scripts.append(arg)
        else:
            paths.append(arg)
        i += 1

    lines = [l.decode() for l in _lines(fs, paths, stdin)]

    out = stdout
    if inplace:
        if len(paths) != 1:
            raise NotImplementedError(f"sed -i with {paths=}")
        out = fs.open(paths[0], "wb")

    for script in scripts:
        # line range selection
        if script[0] == "$":
            # last line
            linerange = slice(len(lines) - 1, len(lines))
            script = script[1:]
        elif script[0].isdigit():
            # single line
            linenostr = "".join(ch.isdigit() and ch or " " for ch in script).split(
                " ", 1
            )[0]
            lineno = int(linenostr)
            script = script[len(linenostr) :]
            linerange = slice(lineno - 1, lineno)
        else:
            # everything
            linerange = slice(0, len(lines))
        # apply the script
        lines[linerange] = _sedscript(script, lines[linerange])

    # pyre-fixme[7]: Expected `str` but got implicit return value of `None`.
    out.write("".join(lines).encode())


def _sedscript(script: str, lines: List[str]) -> List[str]:
    """run sed script on lines"""
    import re

    if script == "d":
        return []
    elif script.startswith("s"):
        delimiter = script[1]
        pat, replace, *rest = script[2:].split(delimiter)
        count = 1
        if "g" in rest:
            count = 0

        return [re.sub(pat, replace, line, count) for line in lines]
    elif script.startswith("/") and script.count("/") > 1:
        pat, rest = script[1:].split("/", 1)
        patre = re.compile(pat)
        newlines = []
        if rest.startswith("i"):
            # insert before match
            insert = rest[1:].replace("\\\n", "").replace("\\n", "\n") + "\n"
            for line in lines:
                if patre.match(line):
                    newlines.append(insert)
                newlines.append(line)
        elif rest == "p":
            # duplicate matched lines
            for line in lines:
                if patre.match(line):
                    newlines.append(line)
                newlines.append(line)
        else:
            raise NotImplementedError(f"sed {script=}")
        return newlines
    else:
        raise NotImplementedError(f"sed {script=}")


@command
def read(args: List[str], stdin: BinaryIO, env: Env) -> int:
    # do not consume the entire stdin
    line = stdin.readline().strip().decode()
    if line:
        for name in args:
            env.setenv(name, line, Scope.SHELL)
        return 0
    else:
        return 1


@command
def source(args: List[str], env: Env):
    # pyre-fixme[9]: env has type `Env`; used as `Optional[Env]`.
    env = env.parent
    code = ""
    for path in args:
        with env.fs.open(path, "rb") as f:
            code += f.read().decode()

    from .interp import interpcode

    try:
        ret = interpcode(code, env)
    except ShellExit:
        raise
    except Exception as e:
        raise RuntimeError(f"cannot source {args}") from e

    return ret.exitcode


@command
def sh(args: List[str], env: Env, stdout: BinaryIO, stderr: BinaryIO, stdin: BinaryIO):
    env = env.nested(Scope.SHELL)
    env.stdin = stdin
    env.stdout = stdout
    env.stderr = stderr
    codelist = []
    i = 0
    shargs = []
    while i < len(args):
        arg = args[i]
        i += 1
        if arg == "-c":
            codelist.append(args[i])
            shargs.append("sh")
            i += 1
        elif not codelist:
            with env.fs.open(arg, "rb") as f:
                codelist.append(f.read().decode())
            shargs.append(arg)
        else:
            shargs.append(arg)

    code = "\n".join(codelist)
    env.args = shargs

    from .interp import interpcode

    return interpcode(code, env).exitcode


@command
def exit(args: List[str], arg0: str):
    code = 0
    if args:
        code = int(args[-1])
    if arg0 == "return":
        raise ShellReturn(code)
    else:
        raise ShellExit(code)


@command
def shift(env: Env, args: List[str]):
    env = env.parentscope(Scope.FUNCTION)
    n = 1
    if args:
        if len(args) != 1:
            raise NotImplementedError(f"shift {args=}")
        n = int(args[0])
    env.args[:] = env.args[0:1] + env.args[1 + n :]


@command
def chmod(args: List[str], fs: ShellFS):
    recursive = False
    if args[:1] == ["-R"]:
        recursive = True
        args = args[1:]
    if len(args) < 2:
        raise NotImplementedError(f"chmod {args=}")
    modestr = args[0]
    if modestr.isnumeric():
        mode = int(modestr, base=8)
        modefunc = lambda m, mode=mode: mode
    else:
        # parse the 'ug+rwx' mini language
        op = "="
        ugo = ""
        rwx = ""
        for ch in modestr:
            if ch in "+-=":
                op = ch
            elif ch in "ugo":
                ugo += ch
            elif ch == "a":
                ugo = "ugo"
            elif ch in "rwxt":
                rwx += ch
            else:
                raise NotImplementedError(f"chmod {modestr=}")

        import stat

        modemap = {
            "ur": stat.S_IRUSR,
            "uw": stat.S_IWUSR,
            "ux": stat.S_IXUSR,
            "gr": stat.S_IRGRP,
            "gw": stat.S_IWGRP,
            "gx": stat.S_IXGRP,
            "or": stat.S_IROTH,
            "ow": stat.S_IWOTH,
            "ox": stat.S_IXOTH,
            "t": stat.S_ISVTX,
        }

        # default ugo is "a"
        ugo = ugo or "ugo"
        # ugo, rwx => mode
        mode = 0
        for what in rwx:
            if what in modemap:
                mode |= modemap[what]
                continue
            for who in ugo:
                mode |= modemap[f"{who}{what}"]

        if op == "+":
            modefunc = lambda m, mode=mode: m | mode
        elif op == "-":
            modefunc = lambda m, mode=mode: m - (m & mode)
        else:
            modefunc = lambda m, mode=mode: mode

    paths = args[1:]
    if recursive:
        paths = [p2 for p in paths for p2 in fs.glob(p)]
    for path in paths:
        origmode = fs.stat(path).st_mode
        newmode = modefunc(origmode)
        fs.chmod(path, newmode)


@command
def cp(args: List[str], fs: ShellFS):
    return _cpormv(args, fs, fs.cp)


@command
def mv(args: List[str], fs: ShellFS):
    return _cpormv(args, fs, fs.mv)


def _cpormv(args: List[str], fs: ShellFS, op):
    paths = [a for a in args if not a.startswith("-")]
    if len(paths) > 1:
        dst = paths[-1]
        for src in paths[:-1]:
            op(src, dst)


@command
def rm(args: List[str], fs: ShellFS):
    paths = [a for a in args if not a.startswith("-")]
    for path in paths:
        fs.rm(path)


@command
def ln(args: List[str], fs: ShellFS):
    symlink = False
    force = False
    if len(args) == 3 and args[0].startswith("-"):
        flags, *args = args
        for flag in flags[1:]:
            if flag == "s":
                symlink = True
            elif flag == "f":
                force = True
            else:
                raise NotImplementedError(f"ln {flags}")
    if len(args) == 2:
        src, dst = args
        if force:
            fs.rm(dst)
        if symlink:
            fs.symlink(src, dst)
        else:
            fs.link(src, dst)
    else:
        raise NotImplementedError(f"ln f{args=}")


@command
def ls(args: List[str], fs: ShellFS):
    entries = []

    def listdir(path: str, listall: bool = False, fs=fs) -> List[str]:
        if listall:
            return fs.listdir(path)
        return [f for f in fs.listdir(path) if not f.startswith(".")]

    listall = False
    for arg in args:
        if arg == "-a":
            listall = True
        elif arg.startswith("-"):
            raise NotImplementedError(f"ls with flag {arg}")
        elif fs.isdir(arg):
            entries += listdir(arg, listall=listall)
        elif fs.exists(arg):
            entries.append(arg)
    if not args:
        entries = listdir("")
    entries = sorted(entries)
    lines = [f"{path}\n" for path in entries]
    return "".join(lines)


@command
def tar(args: List[str], fs: ShellFS):
    supportedopts = ["C", "f", "x"]
    expargs = []
    for arg in args:
        if arg.startswith("-"):
            for subarg in arg[1:]:
                expargs.append(f"-{subarg}")
        else:
            expargs.append(arg)

    def parseargs(args: List[str]) -> Dict[str, Optional[str]]:
        argsdict: Dict[str, Optional[str]] = {}
        i = 0
        while i < len(args):
            arg = args[i]
            if arg.startswith("-"):
                val = None
                if i + 1 < len(args) and not args[i + 1].startswith("-"):
                    i += 1
                    val = args[i]
                argsdict[arg[1:]] = val
            i += 1
        return argsdict

    def extracttar(filename: str, target: Optional[str]):
        if target is None:
            target = "./"
        with tarfile.open(filename) as tar:
            tar.extractall(target)

    opts = parseargs(expargs)
    for opt in opts.keys():
        if opt not in supportedopts:
            raise NotImplementedError(f"tar with option {opt}")

    if expargs[0] in {"-c", "-r", "-t", "-u"}:
        raise NotImplementedError(f"tar with option {expargs[0]}")
    elif expargs[0] == "-x":
        opts.pop("x")
        filename = opts.pop("f", "")
        if not filename:
            raise RuntimeError(f"-f option must be specified for tar -x")
        target = None
        if "C" in opts:
            target = opts.pop("C")
        if len(opts) > 0:
            raise RuntimeError(f"unsupported options for tar -x: {args}")
        extracttar(filename, target)
    else:
        raise RuntimeError("first option for tar must be one of [-c, -r, -t, -u, -x]")


@command
def mkdir(args: List[str], fs: ShellFS):
    for arg in args:
        if arg.startswith("-"):
            continue
        fs.mkdir(arg)


@command
def chdir(args: List[str], env: Env, fs: ShellFS):
    if args:
        path = args[-1]
    else:
        path = env.getenv("HOME")
    if path:
        fs.chdir(path)


@command
def pwd(fs: ShellFS):
    return "%s\n" % fs.cwd()


@command
def grep(args: List[str], arg0: str, stdin: BinaryIO, fs: ShellFS, stdout: BinaryIO):
    import re

    inverse = False
    only = False
    extended = arg0 == "egrep"

    while args[0].startswith("-"):
        flag, *args = args
        if flag == "-v":
            inverse = True
        elif flag == "-e":
            extended = True
            arg0 = "egrep"
        elif flag == "-o":
            only = True
        elif flag == "--":
            break
        else:
            raise NotImplementedError(f"grep flag {flag}")

    # unlike egrep, grep does not treat "(" or ")" specially
    patstr = args[0]
    if not extended:
        patstr = patstr.replace("(", r"\(").replace(")", r"\)")

    pat = re.compile(patstr)
    paths = args[1:]
    lines = [l.decode() for l in _lines(fs, paths, stdin)]
    line_matches = [(l, pat.search(l)) for l in lines]
    if only:
        out = "".join(
            f"{m.group()}\n" for l, m in line_matches if m and bool(m) != inverse
        )
    else:
        out = "".join(l for l, m in line_matches if bool(m) != inverse)
    stdout.write(out.encode())
    if not out:
        return 1


@command
def sort(args: List[str], stdin: BinaryIO, fs: ShellFS):
    paths = args
    lines = [l.decode() for l in _lines(fs, paths, stdin)]
    lines = sorted(lines)
    return "".join(lines)


@command
def find(args: List[str], fs: ShellFS):
    i = 0
    findpaths = []
    filters = []
    negate = False
    origcwd = fs.cwd()

    def appendfilter(func):
        nonlocal negate
        if negate:
            filters.append(lambda p: not func(p))
            negate = False
        else:
            filters.append(func)

    while i < len(args):
        arg = args[i]
        i += 1
        if arg == "-type":
            typestr = args[i]
            i += 1
            if typestr == "f":
                appendfilter(lambda p: fs.isfile(p))
            elif typestr == "d":
                appendfilter(lambda p: fs.isdir(p))
            else:
                raise NotImplementedError(f"find -type {typestr}")
        elif arg == "-perm":
            modestr = args[i]
            i += 1
            if modestr.startswith("-"):
                mode = int(modestr[1:], 8)
                appendfilter(lambda p: (fs.stat(p).st_mode & mode) == mode)
            else:
                raise NotImplementedError(f"find -perm {modestr}")
        elif arg == "-not":
            negate = True
        elif arg == "-wholename":
            patstr = args[i]
            i += 1

            from fnmatch import fnmatch

            appendfilter(lambda p, pat=patstr: fnmatch(p, pat))
        elif arg.startswith("-"):
            raise NotImplementedError(f"find {arg}")
        elif arg == ".":
            findpaths.append("")
        else:
            findpaths.append(arg)

    outpaths = []
    for findpath in findpaths:
        fs.chdir(origcwd)
        if findpath:
            fs.chdir(findpath)
        prefix = findpath and f"{findpath}/" or ""
        paths = [f"{prefix}{p}" for p in fs.glob("**/*")]
        fs.chdir(origcwd)
        paths = [p for p in paths if all(f(p) for f in filters)]
        outpaths += paths

    fs.chdir(origcwd)

    return "".join(f"{p}\n" for p in outpaths)


@command
def wc(args: List[str], stdin: BinaryIO, fs: ShellFS):
    if args[0] == "-l":
        linecounter = lambda l: 1
    elif args[0] == "-c":
        linecounter = lambda l: len(l)
    else:
        raise NotImplementedError(f"wc {args}")
    count = sum(linecounter(l) for l in _lines(fs, args[1:], stdin))
    return f"{count}\n"


@command
def py(args: List[str], stdout: BinaryIO):
    for name in args:
        result = _lookup_python(name)
        if result is not None:
            stdout.write(f"{result}\n".encode())


@command
def sleep(args: List[str]):
    if len(args) != 1:
        raise NotImplementedError(f"sleep {args}")
    duration = float(args[0])

    import time

    time.sleep(duration)


def _lookup_python(name):
    """lookup Python variable name from the Python stack"""
    f = sys._getframe(1)
    nothing = object()
    while f is not None:
        skip = f.f_globals.get("SKIP_PYTHON_LOOKUP")
        if not skip:
            for variables in f.f_locals, f.f_globals:
                result = variables.get(name, nothing)
                if result is not nothing:
                    return result
        f = f.f_back
    f = None
    return None


def _parseheadtail(args) -> Tuple[int, List[str]]:
    """parse the -n parameter for head and tail
    return (n, paths)
    """
    n = 10
    paths = []
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "-n":
            # -n 1
            n = int(args[i + 1])
            i += 2
        elif arg.startswith("-n"):
            # -n1
            n = int(arg[2:])
            i += 1
        elif arg.startswith("-"):
            # -1
            n = int(arg[1:])
            i += 1
        else:
            paths.append(arg)
            i += 1
    return (n, paths)


def _lines(
    fs: ShellFS,
    paths: List[str],
    stdin: Optional[BinaryIO] = None,
    reporterror: Optional[Callable[[str, str], None]] = None,
) -> Iterator[bytes]:
    """yield lines in paths and stdin"""
    if not paths:
        paths = ["-"]
    for path in paths:
        if path == "-":
            if stdin:
                yield from stdin
        else:
            try:
                with fs.open(path, "rb") as f:
                    yield from f
            except FileNotFoundError:
                if reporterror is None:
                    raise
                reporterror(path, "No such file or directory")
            except NotADirectoryError:
                if reporterror is None:
                    raise
                reporterror(path, "Not a directory")


cmdtable["["] = cmdtable["[["] = cmdtable["test"]
cmdtable["."] = cmdtable["source"]
cmdtable[":"] = cmdtable["true"]
cmdtable["return"] = cmdtable["exit"]

cmdtable["cd"] = cmdtable["chdir"]
cmdtable["rmdir"] = cmdtable["unlink"] = cmdtable["rm"]
cmdtable["egrep"] = cmdtable["grep"]
