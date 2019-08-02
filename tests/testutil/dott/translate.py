# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Translate .t tests to .py tests

Translate result might need extra manual editing to work.

To run this script in parallel:

    echo *.t | xargs -P9 -n1 python -m testutil.dott.translate --black --verify
"""

from __future__ import absolute_import

import ast
import hashlib
import os
import re
import shlex
import subprocess
import sys

from edenscm.mercurial import util

from .. import autofix


_repr = autofix._repr


def shellquote(s, _needsshellquote=re.compile(br"[^a-zA-Z0-9._/+-]").search):
    if _needsshellquote(s):
        s = "'%s'" % s.replace("'", "'\\''")
    return s


def parsecmd(line):
    argiter = iter(shlex.split(line))
    nextarg = lambda: next(argiter, None)
    opts = {}
    cmd = []

    def reset(opts=opts, cmd=cmd):
        for name in [">>", "<<", ">", "<"]:
            opts[name] = None
        cmd[:] = []

    def result():
        return " ".join(map(shellquote, cmd)), opts.copy()

    reset()

    while True:
        arg = nextarg()
        if arg is None or arg == "#":
            break
        if arg in {";", "&&"}:
            yield result()
            reset()
            continue
        for name in [">>", "<<", ">", "<"]:
            if arg.startswith(name):
                if len(arg) > len(name):
                    opts[name] = arg[len(name) :]
                else:
                    opts[name] = nextarg()
                break
        else:
            cmd.append(arg)
    yield result()


def translatecontent(code, state):
    """Translate dott code. Return (python code, rest dott code)"""
    if not code:
        return "", code

    firstline, rest = code.split("\n", 1)
    state["indent"] = state.get("nextindent", 0)

    if not firstline:
        # new line
        return "\n", rest
    elif firstline.startswith("#require "):
        return "feature.require(%r)\n" % (firstline[9:].split(),), rest
    elif firstline.startswith("#if "):
        state["nextindent"] += 4
        return "if feature.check(%r):\n" % (firstline[4:].split(),), rest
    elif firstline.startswith("#else"):
        state["indent"] -= 4
        return "else:\n", rest
    elif firstline.startswith("#endif"):
        state["indent"] -= 4
        state["nextindent"] -= 4
        return "\n", rest
    elif firstline.startswith("#testcases "):
        state["nextindent"] += 4
        return "for testcase in %r:\n" % (firstline[11:].split(),), rest
    elif firstline[0:2] != "  ":
        # comment
        message = firstline
        if not message.startswith("#"):
            message = "# " + message
        return "%s\n" % message, rest
    elif firstline.startswith("  >>> ") or firstline.startswith("  ... "):
        # inline python
        return "%s\n" % firstline[6:], rest
    elif firstline.startswith("  $ "):
        # shell command
        allcode = ""
        parsed = list(parsecmd(firstline[4:]))
        for cmd, opts in parsed:
            code = "sh %% %r" % cmd
            # Be careful with Python operator precedence and auto-rewrite
            # from `a op1 b op2 c` to `a op1 b and b op2 c`. Strategy:
            # - Do not use `<`. Avoid `cmd < in > out` being written to
            #   `cmd < in and in > out`.
            # - Input first. Use `cmd << in > out` instead of `cmd > out << in`.
            redirects = []
            # stdin
            if opts["<"]:
                redirects.append("<< open(%r).read()" % opts["<"])
            elif opts["<<"]:
                heredoc, rest = _scanheredoc(rest)
                heredoc = _marktrailingspaces(heredoc)
                redirects.append("<< %s" % _repr(heredoc, indent=0))
            # stdout
            if opts[">"]:
                redirects.append("> %r" % opts[">"])
            elif opts[">>"]:
                redirects.append(">> %r" % opts[">>"])
            code = " ".join([code] + redirects)
            # compare output for the last command
            if cmd is parsed[-1][0]:
                output, rest = _scanoutput(rest)
                output = _marktrailingspaces(output.rstrip())
                if output:
                    code += " == %s" % _repr(output, indent=4)
            allcode += code + "\n"
        return allcode, rest

    return "", code


def _marktrailingspaces(text):
    """append '(trailing space)' to lines with trailing spaces"""
    if "\n" in text:
        lines = text.splitlines(True)
        newtext = ""
        for line in lines:
            if line.endswith("\n"):
                if line[-2:-1].isspace():
                    line = line[:-1] + " (trailing space)\n"
            else:
                if line[-1:].isspace():
                    line += " (trailing space)"
            newtext += line
        return newtext
    else:
        return text


def _scanoutput(content):
    """Capture the output part. Return (output, rest)"""
    output = ""
    count = 0
    lines = content.splitlines(True)
    for line in lines:
        if line.startswith("  ") and line[2:4] not in {"$ ", ">>"}:
            output += line[2:]
            count += 1
        else:
            break
    return output, "".join(lines[count:])


def _scanheredoc(content):
    """Capture the heredoc part. Return (output, rest)"""
    heredoc = []
    lines = content.splitlines(True)
    for line in lines:
        if line.startswith("  > "):
            heredoc.append(line[4:])
        else:
            break
    return "".join(heredoc[:-1]), "".join(lines[len(heredoc) :])


def translatepath(path, black=False, verify=False):
    header = "# Copyright (c) Facebook, Inc. and its affiliates.\n"
    if not _iscreatedbyfb(path):
        header += "# Copyright (c) Mercurial Contributors.\n"
    header += r"""#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


"""
    code = open(path).read()
    body = translatebody(code)
    newpath = "%s-t.py" % path[:-2]
    with open(newpath, "w") as f:
        f.write(header + body)
    if verify:
        # Run the test. Skip it if it fails.
        # Store error message in .err.
        errpath = newpath + ".err"
        if os.system("python %r &> %s" % (newpath, errpath)) != 0:
            skipcode = "feature.require('false')  # test not passing\n"
            with open(newpath, "w") as f:
                f.write(header + skipcode + body)
        else:
            os.unlink(errpath)
    if black:
        os.system("black %r > %s" % (newpath, os.devnull))


def translatebody(code):
    body = ""
    # hack: make multi-line command single-line
    code = code.replace("\\\n  > ", "")
    state = {"indent": 0, "nextindent": 0}
    while code:
        try:
            newcode, rest = translatecontent(code, state)
            indent = " " * state.get("indent", 0)
        except Exception as ex:
            print("  (exception %r at %r)" % (ex, code.split("\n", 1)[0]))
            return
        if len(rest) == len(code) and not newcode:
            print("  (failed at %r)" % code.split("\n", 1)[0])
            return
        for line in newcode.splitlines(True):
            if line not in {"\n", ""} and indent:
                line = indent + line
            body += line
        code = rest
    return body


def _iscreatedbyfb(path):
    """Returns True if path was created by FB.

    This function is very slow. So it uses ~/.cache/testutil/authordb/ as cache.
    """
    cachepath = os.path.expanduser(
        "~/.cache/testutil/authordb/%s" % hashlib.sha1(path).hexdigest()
    )
    if not os.path.exists(cachepath):
        util.makedirs(os.path.dirname(cachepath))
        lines = sorted(
            subprocess.check_output(
                ["hg", "log", "-f", "-T{author|email}\n", path]
            ).splitlines()
        )
        result = all(l.endswith("@fb.com") for l in lines)
        open(cachepath, "w").write(repr(result))
    return ast.literal_eval(util.readfile(cachepath))


def main(argv):
    verify = "--verify" in argv
    black = "--black" in argv
    for path in argv:
        if path.endswith(".t"):
            print("Translating %s" % path)
            translatepath(path, verify=verify, black=black)


if __name__ == "__main__":
    main(sys.argv[1:])
