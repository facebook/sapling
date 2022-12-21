# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""conch-parser AST -> InterpResult

See module-level doc for examples.
"""

import functools
import os
import re
import shlex
import textwrap
import threading
import traceback
from dataclasses import dataclass
from functools import partial
from io import BytesIO
from typing import List, Optional

# pyre-fixme[21]: Could not find module `conch_parser`.
import conch_parser

from .types import Env, InterpResult, OnError, Scope, ShellExit, ShellReturn

SKIP_PYTHON_LOOKUP = True


def sheval(code, env: Env, onerror=OnError.RAISE) -> str:
    """parse and interpret shell logic"""
    res = interpcode(code, env, onerror=onerror)
    # convert to '.t'-friendly representation
    out = res.out
    if out and not out.endswith("\n"):
        out += " (no-eol)\n"
    if res.exitcode:
        out += f"[{res.exitcode}]\n"
    return out


def interpcode(code: str, env: Env, onerror=OnError.RAISE) -> InterpResult:
    try:
        trees = conch_parser.parse(code)
    except Exception as e:
        raise ValueError(f"cannot parse shell code: {code}") from e
    try:
        return interpvec(trees, env, onerror=onerror)
    except ShellExit as e:
        return e.result()


def interp(tree: dict, env: Env) -> InterpResult:
    """Interpret an AST node - main entry point"""
    t = tree["t"]
    interpfunc = INTERP_TYPE_TABLE.get(t)
    if interpfunc is None:
        raise NotImplementedError(f"interp {t}")
    v = tree.get("v")
    result = interpfunc(v, env)
    assert isinstance(result, InterpResult), f"interp {t} returned wrong type"
    return result


def interpvec(trees, env: Env, onerror=OnError.RAISE) -> InterpResult:
    """Interprete a list of ASTs and chain their result together"""
    res = InterpResult()
    for tree in trees:
        try:
            nextres = interp(tree, env)
        except ShellExit as e:
            # re-raise with incomplete InterpResult
            if e.res:
                res = res.chain(e.res)
            e.res = res
            raise
        except Exception:
            if onerror == OnError.RAISE:
                raise
            else:
                res.out += "# Python Exception in Shell interpreter:\n"
                res.out += "# Stack\n"
                res.out += textwrap.indent("".join(traceback.format_stack()[:-1]), "# ")
                res.out += textwrap.indent(traceback.format_exc(), "# ")
                if onerror == OnError.WARN_ABORT:
                    e = ShellExit(127)
                    e.res = res
                    raise e
                else:
                    assert onerror == OnError.WARN_CONTINUE
        else:
            res = res.chain(nextres)
    return res


def interpliteral(v, env: Env, quoted: Optional[str] = None) -> InterpResult:
    return InterpResult(out=str(v), quoted=quoted)


def interpfixed(out: str, v, env: Env) -> InterpResult:
    return InterpResult(out=out)


def interpdoublequote(v, env: Env) -> InterpResult:
    # v: Vec<DefaultSimpleWord>
    words = []
    quoted = '"'
    for tree in v:
        # Special case: do not treat "$@" as quoted.
        if tree.get("t") == "Param" and tree["v"]["t"] == "At":
            quoted = None
        out = interp(tree, env).out
        words.append(out)
    return InterpResult(out="".join(words), quoted=quoted)


def interplist(v, env: Env) -> InterpResult:
    # List of chained commands (ex. a && b || c)
    res = interp(v["first"], env)
    for tree in v["rest"]:
        t = tree["t"]
        v = tree["v"]
        assert t in ("And", "Or")
        if (t == "And") == (res.exitcode == 0):
            nextres = interp(v, env)
            res = res.chain(nextres)
    return res


def interpjob(v, env: Env) -> InterpResult:
    origenv = env
    env = env.nested(Scope.SHELL)
    # To avoid race conditions, forbid input and capture output by default so
    # they can show as 'wait' output.
    env.stdin = BytesIO()
    env.stdout = env.stderr = out = BytesIO()
    thread = threading.Thread(target=interplist, args=(v, env), daemon=True)
    origenv.jobs.append((thread, out))
    thread.start()
    return InterpResult()


def interpenvvar(v, env: Env, scope: Optional[Scope]) -> InterpResult:
    name = v[0]
    if v[1]:
        out = interp(v[1], env).out
    else:
        out = ""
    env.setenv(name, out, scope)
    if scope == Scope.COMMAND:
        # 'A=1 command' - command gets 'A' without explicit export
        env.exportenv(name)
    return InterpResult()


def interpvar(v, env: Env) -> InterpResult:
    out = env.getenv(v)
    return InterpResult(out=out, quoted="$")


def interpdefault(v, env: Env) -> InterpResult:
    out = interp(v[1], env).out or interp(v[2], env).out
    return InterpResult(out=out)


def interpargs(trees, env: Env) -> List[str]:
    args = []
    for tree in trees:
        res = interp(tree, env)
        # Expand ~ first.
        if not res.quoted and (res.out == "~" or res.out.startswith("~/")):
            home = env.getenv("HOME")
            if home:
                res.out = home + res.out[1:]
                # emulate $HOME expansion
                res.quoted = res.quoted or "$"
        # Expand globs.
        if res.quoted in {"$", None} and "*" in res.out:
            matched = env.fs.glob(res.out)
            if matched:
                args += matched
                continue
        # Expand space-separated words, or handle quotes.
        # note about shlex.split:
        # input       | shlex.split(posix=True) | shlex.split(posix=False)
        # r'C:\Users' | ['C:Users']             | [r'C:\Users']
        # '"a  b"'    | ['a  b']                | ['"a  b"']
        if res.quoted in {'"', "'"}:
            args.append(res.out)
        elif res.quoted in {"`", "$"}:
            args += shlex.split(res.out, posix=False)
        else:
            assert res.quoted is None, f"unsupported {res.quoted=}"
            args += shlex.split(res.out)
    return args


def interpsubst(v, env: Env) -> InterpResult:
    env = env.nested(Scope.SHELL)
    env.stdout = BytesIO()
    res = interp(v, env)
    # pyre-fixme[16]: `Optional` has no attribute `getvalue`.
    res.out += env.stdout.getvalue().decode()
    res.out = res.out.rstrip()
    res.quoted = "`"
    return res


def interpsimplecommand(v, env: Env) -> InterpResult:
    # avoid affecting parent env since we might change redirect or env.
    env = env.nested(Scope.COMMAND)

    # resolve args without applying env changes.
    # 'A=1 echo $A' will resolve $A first before applying A=1,
    # the 'echo' command will get exported A=1.
    args = interpargs(v["redirects_or_cmd_words"], env)

    # special case: translate "> foo" to "true > foo"
    if not args and any(t["t"] == "Redirect" for t in v["redirects_or_env_vars"]):
        args = ["true"]

    if not args:
        # ex. A=1 (without command) - apply env to the shell or local
        envscope = None
    else:
        # ex. A=1 command - apply env to just the command
        envscope = Scope.COMMAND

    # apply env changes
    for tree in v["redirects_or_env_vars"]:
        if tree["t"] == "EnvVar":
            interpenvvar(tree["v"], env, envscope)
        else:
            assert tree["t"] == "Redirect"
            interp(tree["v"], env)

    if not args:
        return InterpResult()

    cmdfunc = env.getcmd(args[0])
    env.args = args
    ret = cmdfunc(env)
    env.lastexitcode = ret.exitcode
    return ret


def interppipe(v, env: Env) -> InterpResult:
    # v: (negate: bool, Vec<T>)
    origenv = env
    negate, trees = v
    res = InterpResult()
    allocatedstdout = allocatedstderr = False
    if len(trees) > 1:
        # has at least one "|", capture stdout
        env = env.nested(Scope.COMMAND)
        env.stdout = BytesIO()
        allocatedstdout = True
        if env.stderr is None:
            env.stderr = BytesIO()
            allocatedstderr = True
    for i, tree in enumerate(trees):
        if i > 0:
            # stdout of previous comamnd is stdin for the next command
            stdin = env.stdout
            # pyre-fixme[16]: `Optional` has no attribute `seek`.
            stdin.seek(0)
            env.stdin = stdin
            env.stdout = BytesIO()
        res = interp(tree, env)
    if negate:
        res.exitcode = int(not res.exitcode)
    if allocatedstdout:
        # pyre-fixme[16]: `Optional` has no attribute `getvalue`.
        out = env.stdout.getvalue()
        if origenv.stdout is None:
            res.out = out.decode() + res.out
        else:
            origenv.stdout.write(out)
    if allocatedstderr:
        err = env.stderr.getvalue()
        if origenv.stderr is None:
            res.out = res.out + err.decode()
        else:
            # pyre-fixme[61]: `out` is undefined, or not always defined.
            origenv.stderr.write(out)
    return res


def interpcompound(v, env: Env) -> InterpResult:
    # v: CompoundCommand
    io = v["io"]
    if io:
        env = env.nested(Scope.COMPOUND)
        interpvec(io, env)

    # kind: CompoundCommandKind
    kind = v["kind"]
    kindt = kind["t"]
    kindv = kind["v"]
    assert kindt in {"Brace", "Subshell", "While", "Until", "If", "Case", "For"}

    if kindt == "Brace":
        return interpvec(kindv, env)
    elif kindt == "Subshell":
        env = env.nested(Scope.SHELL)
        try:
            return interpvec(kindv, env)
        except ShellExit as e:
            return e.result()
    elif kindt == "If":
        conditionals = kindv["conditionals"]  # GuardBodyPair
        body = kindv["else_branch"] or []
        for cond in conditionals:
            guard = cond["guard"]  # Vec<C>
            guardres = interpvec(guard, env)
            if guardres.exitcode == 0:
                body = cond["body"]  # Vec<C>
                break
        return interpvec(body, env)
    elif kindt == "While":
        res = InterpResult()
        while True:
            guard = kindv["guard"]  # Vec<C>
            guardres = interpvec(guard, env)
            if guardres.exitcode != 0:
                break
            body = kindv["body"]  # Vec<C>
            res = res.chain(interpvec(body, env))
        return res
    elif kindt == "For":
        varname = kindv["var"]
        words = kindv["words"]
        body = kindv["body"]
        values = interpargs(words, env)
        res = InterpResult()
        env = env.nested(Scope.COMPOUND)
        for value in values:
            env.setenv(varname, value, Scope.COMPOUND)
            bodyres = interpvec(body, env)
            res = res.chain(bodyres)
        return res
    elif kindt == "Case":
        wordtree = kindv["word"]  # W
        arms = kindv["arms"]  # Vec<PatternBodyPair>
        wordres = interp(wordtree, env)
        word = wordres.out.strip()
        for arm in arms:
            pats = arm["patterns"]
            for pat in pats:
                # NOTE: not a real pattern match
                if interp(pat, env).out.strip() == word:
                    body = arm["body"]
                    return interpvec(body, env)
        return InterpResult()

    raise NotImplementedError(f"compound {kindt}")


def interpredirect(v, env: Env, mode: str, defaultfd: int = 0) -> InterpResult:
    # v: ex. Write(Option<u16>, W)
    fd = v[0]
    if fd is None:
        fd = defaultfd
    path = interp(v[1], env).out
    f = env.fs.open(path, mode)
    if fd == 0:
        env.stdin = f
    elif fd == 1:
        env.stdout = f
    elif fd == 2:
        env.stderr = f
    else:
        raise NotImplementedError(f"redirect with {fd=}")
    return InterpResult()


def interpdupwrite(v, env: Env) -> InterpResult:
    # v: ex. DupWrite(Option<u16>, W)
    fd = v[0]
    if fd is None:
        fd = 1
    destfd = int(interp(v[1], env).out)
    if fd == destfd:
        pass
    elif (fd, destfd) == (2, 1) and env.stdout:
        env.stderr = env.stdout
    elif (fd, destfd) == (1, 2) and env.stderr:
        env.stdout = env.stderr
    else:
        raise NotImplementedError(
            f"dup {fd} -> {destfd} while fd {destfd} is not already redirected"
        )
    return InterpResult()


def interpheredoc(v, env: Env) -> InterpResult:
    # v: Heredoc(Option<u16>, W)
    fd = v[0]
    if fd not in {None, 1}:
        raise NotImplementedError(f"heredoc with {fd=}")
    res = interp(v[1], env)
    # Set res.out as the stdin
    env.stdin = BytesIO(res.out.encode())
    return InterpResult()


def interpfunctiondef(v, env: Env) -> InterpResult:
    # v: FunctionDef(N, F), F=Compound
    name = v[0]
    body = v[1]
    shfunc = ShellFunction(name=name, compound=body)
    env.cmdtable[name] = shfunc
    return InterpResult()


@dataclass
class ShellFunction:
    name: str
    compound: dict

    def __call__(self, env: Env) -> InterpResult:
        env = env.nested(Scope.FUNCTION)
        try:
            return interpcompound(self.compound, env)
        except ShellReturn as e:
            return e.result()

    def __repr__(self):
        return f"<ShellFunction '{self.name}'>"


def interppositional(v, env: Env) -> InterpResult:
    index = v
    out = ""
    if index < len(env.args):
        out = env.args[index]
    return InterpResult(out=out)


def interppound(v, env: Env) -> InterpResult:
    out = str(max(len(env.args), 1) - 1)
    return InterpResult(out=out)


def interpquestionparameter(v, env: Env) -> InterpResult:
    out = str(env.lastexitcode)
    return InterpResult(out=out)


def interpat(v, env: Env) -> InterpResult:
    out = shlex.join(env.args[1:])
    return InterpResult(out=out)


def interpmath(v, env: Env, func) -> InterpResult:
    # v: Add(Arithmetic, Arithmetic)
    values = (int(interp(tree, env).out or "0") for tree in v)
    value = functools.reduce(func, values)
    out = str(int(value))
    return InterpResult(out=out)


def interpremovelargestprefix(v, env: Env) -> InterpResult:
    # v: RemoveLargestPrefix(P, Option<W>)
    value = interp(v[0], env).out
    globpat = interp(v[1], env).out or ""
    repat = re.escape(globpat).replace(r"\*", ".*")
    matched = re.match(repat, value)
    if matched:
        value = value[matched.end() :]
    return InterpResult(out=value)


# interp based on tree node type
#
# see enum variant names in conch-parser's ast.rs
# use 'b.shparser.parse(code)' in debugshell to view parsed dict for code.
# None means not yet implemented.
INTERP_TYPE_TABLE = {
    # Command
    "List": interplist,
    "Job": interpjob,
    # ListableCommand
    "Pipe": interppipe,
    "SingleCommand": interp,
    "SimpleCommand": interpsimplecommand,
    "Compound": interpcompound,
    "FunctionDef": interpfunctiondef,
    # ComplexWord
    "SingleWord": interp,
    "Concat": interpvec,
    # Word
    "SimpleWord": interp,
    "DoubleQuoted": interpdoublequote,
    "SingleQuoted": partial(interpliteral, quoted="'"),
    # Parameter
    "At": interpat,
    "StarParameter": None,
    "Pound": interppound,
    "QuestionParameter": interpquestionparameter,
    "Dash": None,
    "Dollar": None,
    "Bang": None,
    "Positional": interppositional,
    "VarParameter": interpvar,
    # RedirectOrCmdWord
    "Redirect": interp,
    "CmdWord": interp,
    # RedirectOrEnvVar
    "EnvVar": None,  # handled by SimpleCommand
    # Redirect
    "Read": partial(interpredirect, mode="rb", defaultfd=0),
    "Write": partial(interpredirect, mode="wb", defaultfd=1),
    "Append": partial(interpredirect, mode="ab", defaultfd=1),
    "Heredoc": interpheredoc,
    "Clobber": None,
    "ReadWrite": None,
    "DupRead": None,
    "DupWrite": interpdupwrite,
    # SimpleWord
    "LiteralWord": interpliteral,
    "Escaped": interpliteral,
    "Param": interp,
    "Subst": interpsubst,
    "StarWord": partial(interpfixed, "*"),
    "QuestionWord": None,
    "SquareOpen": partial(interpfixed, "["),
    "SquareClose": partial(interpfixed, "]"),
    "Tilde": partial(interpfixed, "~"),
    "Colon": partial(interpfixed, ":"),
    # ParameterSubstitution
    "Command": interpvec,
    "Len": None,
    "Arith": interp,
    "Default": interpdefault,
    "AssignSubstitution": None,
    "Error": None,
    "Alternative": None,
    "RemoveSmallestSuffix": None,
    "RemoveLargestSuffix": None,
    "RemoveSmallestPrefix": None,
    "RemoveLargestPrefix": interpremovelargestprefix,
    # Arithmetic
    "VarArithmetic": interpvar,
    "LiteralArithmetic": interpliteral,
    "Pow": partial(interpmath, func=int.__pow__),
    "PostIncr": None,
    "PostDecr": None,
    "PreIncr": None,
    "PreDecr": None,
    "UnaryPlus": None,
    "UnaryMinus": None,
    "LogicalNot": None,
    "BitwiseNot": None,
    "Mult": partial(interpmath, func=int.__mul__),
    "Div": partial(interpmath, func=int.__floordiv__),
    "Modulo": partial(interpmath, func=int.__mod__),
    "Add": partial(interpmath, func=int.__add__),
    "Sub": partial(interpmath, func=int.__sub__),
    "ShiftLeft": partial(interpmath, func=int.__lshift__),
    "ShiftRight": partial(interpmath, func=int.__rshift__),
    "Less": partial(interpmath, func=int.__lt__),
    "LessEq": partial(interpmath, func=int.__le__),
    "Great": partial(interpmath, func=int.__gt__),
    "GreatEq": partial(interpmath, func=int.__ge__),
    "Eq": partial(interpmath, func=int.__eq__),
    "NotEq": partial(interpmath, func=int.__ne__),
    "BitwiseAnd": partial(interpmath, func=int.__and__),
    "BitwiseXor": partial(interpmath, func=int.__xor__),
    "BitwiseOr": partial(interpmath, func=int.__or__),
    "LogicalAnd": None,
    "LogicalOr": None,
    "Ternary": None,
    "AssignArithmetic": None,
    "Sequence": None,
}
