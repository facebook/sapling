# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""'.t' test to Python code transformer

Parse and rewrite '.t' test code in Python.

Syntax rules:

  - Indented ">>>" and "$" blocks are Python or bash code,
    followed by output to test, similar to core `.t` features:

        $ echo 1
        1

        >>> 1 + 1
        2

  - Code block (identified by identation, or separated by blank lines)
    in valid Python is treated as Python code without testing output:

        if True:
            pass

  - Python code block can embed "$" or ">>>" block:

        if True:
            $ echo 1
            1

  - Other things are treated as comments:

        this line is a comment but the 'echo 1' will be tested:
            $ echo 1
            1

        this line is a comment but the Python code below will be executed
        (note the empty line is needed):

            def foo():
                return 1

The main function is transform()
"""
from __future__ import annotations

import textwrap
from ast import parse
from dataclasses import dataclass
from typing import Callable, List, Optional


def transform(
    code: str,
    indent: int = 0,
    prefix: str = "",
    filename: str = "",
    hasfeature: Optional[Callable[[str], bool]] = None,
) -> str:
    r"""transform .t test code to Python code

    Example:

        >>> print(transform('''begin of .t test:
        ...   $ echo 1
        ...   1
        ...
        ... if True:  # mixed in python code
        ...   $ echo 2
        ...   2
        ...
        ... this is a comment:
        ...     class A:
        ...         pass
        ... end of python code
        ... end of .t test''', filename='a.t'))
        # begin of .t test:
        <BLANKLINE>
        checkoutput(sheval('echo 1\n'), '1\n', src='$ echo 1\n', srcloc=1, outloc=2, endloc=3, indent=2, filename='a.t')
        <BLANKLINE>
        if True:  # mixed in python code
        <BLANKLINE>
          checkoutput(sheval('echo 2\n'), '2\n', src='$ echo 2\n', srcloc=5, outloc=6, endloc=7, indent=2, filename='a.t')
        <BLANKLINE>
        # this is a comment:
        class A:
            pass
        # end of python code
        # end of .t test

    With indent:

        >>> print(transform('a = 1', indent=2))
          a = 1

    With indent, prefix and filename:

        >>> print(transform(r'''
        ... #require foo
        ... #if foo
        ...     if True:
        ...         pass
        ...     $ echo 1
        ...     1
        ...     >>> print(2)
        ...     2
        ... #endif
        ... ''',
        ...     indent=4,
        ...     prefix='self.',
        ...     filename='a.py',
        ...     hasfeature=lambda f: True))
        <BLANKLINE>
            #if foo
        <BLANKLINE>
            if True:
                pass
        <BLANKLINE>
            self.checkoutput(self.sheval('echo 1\n'), '1\n', src='$ echo 1\n', srcloc=5, outloc=6, endloc=7, indent=4, filename='a.py')
            self.checkoutput(self.pydoceval('print(2)\n') or '', '2\n', src='>>> print(2)\n', srcloc=7, outloc=8, endloc=9, indent=4, filename='a.py')
        <BLANKLINE>
            #endif
        <BLANKLINE>
    """
    code = rewriteblocks(code, prefix=prefix, filename=filename, hasfeature=hasfeature)
    code = commentinvalid(code)
    if indent:
        code = textwrap.indent(code, " " * indent)
    return code


@dataclass
class LineInfo:
    line: str
    indent: int
    prompt: str

    @classmethod
    def fromline(cls, line: str) -> LineInfo:
        """construct LineInfo from a plain line content"""
        stripped = line.lstrip(" ")
        indent = len(line) - len(stripped)
        prompt = stripped.split(" ", 1)[0]
        return cls(line, indent, prompt)

    def stripindent(self, info: LineInfo) -> str:
        """line content without indent"""
        return info.line[self.indent :]

    def stripprompt(self, info: LineInfo) -> str:
        """line content without prompt ('  $')"""
        return info.line[self.indent + len(self.prompt) + 1 :]

    def isfollowprompt(self, info: LineInfo) -> bool:
        """test if self belongs to multi-line input starting from 'info'"""
        return info.indent == self.indent and (info.prompt, self.prompt) in (
            ("$", ">"),
            (">>>", "..."),
        )

    def isfollowoutput(self, info: LineInfo) -> bool:
        """test if self belongs to multi-line ouptut matching 'info'"""
        return info.indent <= self.indent and (
            self.indent != info.indent or self.prompt not in ("$", ">>>")
        )


def rewriteblocks(
    code: str,
    prefix: str = "",
    filename: str = "",
    hasfeature: Optional[Callable[[str], bool]] = None,
) -> str:
    r"""rewrite "obvious" blocks (ex. '$', '>>>') to python code

    prefix is the Python code that resovles to an object with the following
    attributes: require, checkoutput, sheval, pydoceval.

    shell block ($) and python block (>>>) are rewritten:

        >>> print(rewriteblocks('''start
        ...   $ false
        ...   [1]
        ...   $ echo 1
        ...   > echo ' 2' '  $ 3'
        ...   1
        ...    2
        ...     $ 3
        ... inline python
        ...   >>> def f():
        ...   ...     return 3
        ...   >>> f()
        ...   3
        ...
        ...   >>> f() + 3
        ...   6
        ... end'''))
        start
        <BLANKLINE>
          checkoutput(sheval('false\n'), '[1]\n', src='$ false\n', srcloc=1, outloc=2, endloc=3, indent=2, filename='')
          checkoutput(sheval("echo 1\necho ' 2' '  $ 3'\n"), '1\n 2\n  $ 3\n', src="$ echo 1\n> echo ' 2' '  $ 3'\n", srcloc=3, outloc=5, endloc=8, indent=2, filename='')
        <BLANKLINE>
        inline python
        <BLANKLINE>
          checkoutput(pydoceval('def f():\n    return 3\n') or '', '', src='>>> def f():\n...     return 3\n', srcloc=9, outloc=11, endloc=11, indent=2, filename='')
          checkoutput(pydoceval('f()\n') or '', '3\n', src='>>> f()\n', srcloc=11, outloc=12, endloc=13, indent=2, filename='')
        <BLANKLINE>
          checkoutput(pydoceval('f() + 3\n') or '', '6\n', src='>>> f() + 3\n', srcloc=14, outloc=15, endloc=16, indent=2, filename='')
        <BLANKLINE>
        end

    macros like #if and #require are rewritten:

        >>> print(rewriteblocks('''#require git no-windows
        ... #require fsmonitor
        ...
        ...   $ echo 1
        ...   1
        ... #if foo
        ...   $ echo 2
        ...   2
        ... #else
        ...   $ echo 3
        ...   3
        ... #endif
        ...   $ echo 4
        ...   4
        ... end''', hasfeature=lambda f: f in ['git', 'fsmonitor']))
          raise __import__("unittest").SkipTest('missing feature: no-windows')
        <BLANKLINE>
          checkoutput(sheval('echo 1\n'), '1\n', src='$ echo 1\n', srcloc=3, outloc=4, endloc=5, indent=2, filename='')
        <BLANKLINE>
        #if foo
        <BLANKLINE>
        #else
        <BLANKLINE>
          checkoutput(sheval('echo 3\n'), '3\n', src='$ echo 3\n', srcloc=9, outloc=10, endloc=11, indent=2, filename='')
        <BLANKLINE>
        #endif
        <BLANKLINE>
          checkoutput(sheval('echo 4\n'), '4\n', src='$ echo 4\n', srcloc=12, outloc=13, endloc=14, indent=2, filename='')
        <BLANKLINE>
        end

    """
    # preprocess - get indent and prompt ('$' or '>>>') info
    lineinfos: List[LineInfo] = list(map(LineInfo.fromline, code.splitlines(True)))
    n = len(lineinfos)

    # translate '$', '>>>' blocks to python code
    newlines = []
    i = 0

    # push a blank line to between block types
    lastblocktype = None

    def maybeseparate(blocktype):
        nonlocal lastblocktype
        if lastblocktype != blocktype and newlines and newlines[-1] != "\n":
            newlines.append("\n")
        lastblocktype = blocktype

    # "#if" upport:
    # - None: no "#if"
    # - True: lines should be taken until #else or #endif
    # - False: lines should be ignored until #else or #endif
    condition = None

    def appendline(line):
        nonlocal condition
        if condition is not False:
            newlines.append(line)

    while i < n:
        info = lineinfos[i]
        nexti = i + 1
        # convert "$" (sh) or ">>>" (py) block
        if info.indent >= 2 and info.prompt in {"$", ">>>"}:
            srcloc = i
            j = i + 1
            while j < n and lineinfos[j].isfollowprompt(info):
                j += 1
            outloc = j
            while j < n and lineinfos[j].isfollowoutput(info):
                j += 1
            endloc = j
            # source code with prompt, unindented
            src = "".join(info.stripindent(l) for l in lineinfos[srcloc:outloc])
            # source code without prompt
            code = "".join(info.stripprompt(l) for l in lineinfos[srcloc:outloc])
            # reference output
            out = "".join(info.stripindent(l) for l in lineinfos[outloc:endloc])
            if info.prompt == "$":
                code = f"{prefix}sheval({repr(code)})"
            else:
                code = f"{prefix}pydoceval({repr(code)}) or ''"
            # checkoutput with output
            code = f"{prefix}checkoutput({code}, {repr(out)}, src={repr(src)}, {srcloc=}, {outloc=}, {endloc=}, indent={info.indent}, filename={repr(filename)})"
            assert "\n" not in code  # \n should be escaped

            maybeseparate("checkoutput")
            appendline(f"{' ' * info.indent}{code}\n")
            nexti = j
        elif info.line.startswith("#require "):
            maybeseparate("require")
            features = info.line[9:].split()
            if hasfeature is None:
                missing = features
            else:
                missing = [f for f in features if not hasfeature(f)]
            if missing:
                msg = f"missing feature: {' '.join(missing)}"
                appendline(f'  raise __import__("unittest").SkipTest({repr(msg)})\n')
        elif info.line.startswith("#if "):
            maybeseparate("if")
            features = info.line[4:].split()
            appendline(info.line)
            if hasfeature and all(hasfeature(f) for f in features):
                condition = True
            else:
                condition = False
        elif info.line == "#else\n":
            maybeseparate("if")
            if condition is not None:
                condition = not condition
            appendline(info.line)
        elif info.line == "#endif\n":
            maybeseparate("if")
            condition = None
            appendline(info.line)
        else:
            if info.line != "\n":
                maybeseparate("unknown")
            appendline(info.line)
        i = nexti

    return "".join(newlines)


def commentinvalid(code: str) -> str:
    r"""comment out code blocks that are not valid Python

    Examples:

        >>> print(commentinvalid('this is a test\nend'))
        # this is a test
        # end

        >>> print(commentinvalid('if True:\n    pass\n\nnot python code'))
        if True:
            pass
        <BLANKLINE>
        # not python code

        >>> print(commentinvalid('''this is a comment:
        ...     class ThisIsPython:
        ...         def b():
        ...             pass
        ...
        ...         def c():
        ...             pass
        ... ----this is a separator----
        ...         def foo():
        ...             return bar()
        ... end of test'''))
        # this is a comment:
        class ThisIsPython:
            def b():
                pass
        <BLANKLINE>
            def c():
                pass
        # ----this is a separator----
        def foo():
            return bar()
        # end of test
    """
    lines = code.splitlines(True)
    lineinfos: List[LineInfo] = list(map(LineInfo.fromline, lines))
    n = len(lineinfos)

    newlines: List[str] = []
    skipping: Optional[LineInfo] = None
    i = 0
    while i < n:
        info = lineinfos[i]
        if info.line == "\n" or info.line.startswith("#"):
            newlines.append(info.line)
            skipping = None
            i += 1
            continue
        if skipping:
            if info.indent == skipping.indent:
                newlines.append(f"# {info.line}")
                i += 1
                continue
            else:
                # pyre-fixme[9]: skipping has type `Optional[LineInfo]`; used as `bool`.
                skipping = False

        # find a Python code block starting from line i, looks like:
        #
        #     def foo(): # line i             --
        #         ...    #                      | found block
        #         ...    #                    --
        #                # line j (empty)
        #     bar = 1    # line j + 1 (same or less indentation, or end of file)
        #
        # or with less indentation:
        #
        #         def foo(): # line i             --
        #             ...    #                      | found block
        #             ...    #                    --
        #     bar = 1        # line j (less or invalid indentation)

        j = i + 1
        seenindents = {info.indent}
        lastindent = info.indent
        while j < n:
            nextinfo = lineinfos[j]
            if nextinfo.line == "\n":
                if j + 1 < n:
                    nextnext = lineinfos[j + 1]
                    if nextnext.indent <= info.indent:
                        break
                    if (
                        nextnext.indent < lastindent
                        and nextnext.indent not in seenindents
                    ):
                        break
                j += 1
                continue
            if nextinfo.indent < info.indent:
                # outside the starting block
                break
            if nextinfo.indent < lastindent and nextinfo.indent not in seenindents:
                # invalid dedent
                break
            j += 1
            lastindent = nextinfo.indent
            seenindents.add(nextinfo.indent)
        # line i..j is a candidate code block
        candidate = textwrap.dedent("".join(lines[i:j]))
        if _ispythoncodeblock(candidate):
            newlines.append(candidate)
            i = j
        else:
            # skip until a blank line, or a different indent
            skipping = info
    return "".join(newlines)


def _ispythoncodeblock(code: str) -> bool:
    """check if code looks like meaningful Python block"""
    # a single word (ex. 'foo') - not practically meaningful
    if code.strip().isalnum():
        return False
    try:
        parse(code)
    except SyntaxError:
        return False
    return True
