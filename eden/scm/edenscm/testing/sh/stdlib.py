# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""standard library for shell

A subset of "standard" coreutils or shell builtin commands.
For .t test specific commands such as "hg", look at t/runtime.py
instead.
"""

from functools import wraps
from io import BytesIO
from typing import BinaryIO, Optional, List, Tuple, Iterator, Dict, Callable

from .types import Env, InterpResult, ShellFS, ShellReturn, ShellExit, Scope

cmdtable = {}


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

    def reportmissing(path):
        nonlocal exitcode
        exitcode = 1
        stderr.write(f"cat: {path}: No such file or directory\n".encode())

    lines = list(_lines(fs, args, stdin, reportmissing=reportmissing))
    stdout.write(b"".join(lines))
    return exitcode


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
    env = env.parent
    code = ""
    for path in args:
        with env.fs.open(path, "rb") as f:
            code += f.read().decode()

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
    verbose = False

    def listdir(path: str, fs=fs) -> List[str]:
        return [f for f in fs.listdir(path) if not f.startswith(".")]

    for arg in args:
        if arg.startswith("-"):
            if arg == "-l":
                verbose = True
            else:
                raise NotImplementedError(f"ls with flag {arg}")
        elif fs.isdir(arg):
            entries += listdir(arg)
        elif fs.exists(arg):
            entries.append(arg)
    if not args:
        entries = listdir("")
    entries = sorted(entries)
    lines = [f"{path}\n" for path in entries]
    return "".join(lines)


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
    extended = arg0 == "egrep"

    while args[0].startswith("-"):
        flag, *args = args
        if flag == "-v":
            inverse = True
        elif flag == "-e":
            extended = True
            arg0 = "egrep"
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
    out = "".join(l for l in lines if bool(pat.search(l)) != inverse)
    stdout.write(out.encode())
    if not out:
        return 1


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
    reportmissing: Optional[Callable[[str], None]] = None,
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
                if reportmissing is None:
                    raise
                reportmissing(path)


cmdtable["["] = cmdtable["[["] = cmdtable["test"]
cmdtable["."] = cmdtable["source"]
cmdtable[":"] = cmdtable["true"]
cmdtable["return"] = cmdtable["exit"]

cmdtable["cd"] = cmdtable["chdir"]
cmdtable["rmdir"] = cmdtable["unlink"] = cmdtable["rm"]
cmdtable["egrep"] = cmdtable["grep"]
