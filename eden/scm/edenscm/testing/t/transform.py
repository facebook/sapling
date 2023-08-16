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

  - Python code blocks are indented by 4 spaces:

        # This is comment:

            if True:  # This is Python code
                pass

  - Non-indented lines are comments.

        This is a comment.
        # This is another comment.

  - Python code block can embed "$" or ">>>" block:

        if True:
            $ echo 1
            1

The main function in this module is transform().
"""
from __future__ import annotations

import textwrap
from dataclasses import dataclass
from typing import Callable, List, Optional, Tuple


def transform(
    code: str,
    indent: int = 0,
    prefix: str = "",
    filename: str = "",
    hasfeature: Optional[Callable[[str], bool]] = None,
    registertestcase: Optional[Callable[[str], None]] = None,
) -> str:
    r"""transform .t test code to Python code

    Example:

        >>> print(transform('''begin of .t test:
        ... shell code block with output test:
        ...   $ cat << EOF
        ...   > 1
        ...   > EOF
        ...   1
        ...
        ... python code block with output test:
        ...   >>> def f():
        ...   ...     print(2)
        ...   ... f()
        ...   2
        ...
        ... python code block without output test:
        ...     class A:
        ...         pass
        ...
        ... python code block with shell code block:
        ...     if True:
        ...         $ echo 2
        ...         2
        ...
        ... end of .t test''', filename='a.t'))
        # begin of .t test:
        # shell code block with output test:
        <BLANKLINE>
        checkoutput(sheval('cat << EOF\n1\nEOF\n'), '1\n', src='$ cat << EOF\n> 1\n> EOF\n', srcloc=2, outloc=5, endloc=6, indent=2, filename='a.t')
        <BLANKLINE>
        # python code block with output test:
        <BLANKLINE>
        checkoutput(pydoceval('def f():\n    print(2)\nf()\n') or '', '2\n', src='>>> def f():\n...     print(2)\n... f()\n', srcloc=8, outloc=11, endloc=12, indent=2, filename='a.t')
        <BLANKLINE>
        # python code block without output test:
        <BLANKLINE>
        class A:
            pass
        <BLANKLINE>
        # python code block with shell code block:
        <BLANKLINE>
        if True:
        <BLANKLINE>
            checkoutput(sheval('echo 2\n'), '2\n', src='$ echo 2\n', srcloc=19, outloc=20, endloc=21, indent=8, filename='a.t')
        <BLANKLINE>
        # end of .t test

    With indent:

        >>> print(transform('Foo bar', indent=2))
          # Foo bar

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
    code = rewriteblocks(
        code,
        prefix=prefix,
        filename=filename,
        hasfeature=hasfeature,
        registertestcase=registertestcase,
    )
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
        """use self.indent to strip spaces of another LineInfo"""
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
    registertestcase: Optional[Callable[[str], None]] = None,
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
        # start
        <BLANKLINE>
        checkoutput(sheval('false\n'), '[1]\n', src='$ false\n', srcloc=1, outloc=2, endloc=3, indent=2, filename='')
        checkoutput(sheval("echo 1\necho ' 2' '  $ 3'\n"), '1\n 2\n  $ 3\n', src="$ echo 1\n> echo ' 2' '  $ 3'\n", srcloc=3, outloc=5, endloc=8, indent=2, filename='')
        <BLANKLINE>
        # inline python
        <BLANKLINE>
        checkoutput(pydoceval('def f():\n    return 3\n') or '', '', src='>>> def f():\n...     return 3\n', srcloc=9, outloc=11, endloc=11, indent=2, filename='')
        checkoutput(pydoceval('f()\n') or '', '3\n', src='>>> f()\n', srcloc=11, outloc=12, endloc=13, indent=2, filename='')
        <BLANKLINE>
        checkoutput(pydoceval('f() + 3\n') or '', '6\n', src='>>> f() + 3\n', srcloc=14, outloc=15, endloc=16, indent=2, filename='')
        <BLANKLINE>
        # end

    macros like #if and #require are evaluated at "compile" time:

        >>> print(rewriteblocks('''#require git no-windows
        ... #require fsmonitor
        ...
        ...   $ echo 1
        ...   1
        ... #if tar
        ...   $ echo 2
        ...   2
        ... #if foo
        ...   $ echo 3
        ...   3
        ... #endif
        ... #else
        ...   $ echo 4
        ...   4
        ... #endif
        ...   $ echo 5
        ...   5
        ... #if no-tar
        ...   $ echo 6
        ...   6
        ... #if foo
        ...   $ echo 7
        ...   7
        ... #endif
        ... #endif
        ... end''', hasfeature=lambda f: f in ['git', 'fsmonitor', 'tar']))
        raise __import__("unittest").SkipTest('missing feature: no-windows')
        <BLANKLINE>
        checkoutput(sheval('echo 1\n'), '1\n', src='$ echo 1\n', srcloc=3, outloc=4, endloc=5, indent=2, filename='')
        <BLANKLINE>
        #if tar
        <BLANKLINE>
        checkoutput(sheval('echo 2\n'), '2\n', src='$ echo 2\n', srcloc=6, outloc=7, endloc=8, indent=2, filename='')
        <BLANKLINE>
        #if foo
        <BLANKLINE>
        #endif
        <BLANKLINE>
        #endif
        <BLANKLINE>
        checkoutput(sheval('echo 5\n'), '5\n', src='$ echo 5\n', srcloc=16, outloc=17, endloc=18, indent=2, filename='')
        <BLANKLINE>
        #if no-tar
        <BLANKLINE>
        #endif
        <BLANKLINE>
        # end

    #testcases and associated #if macros are evaluated at runtime

        >>> print(rewriteblocks('''#testcases case1 case2
        ... #if case1
        ...   $ echo case1
        ... #else
        ...   $ echo not case1
        ... #endif
        ... #if case2
        ...   $ echo case2
        ... #endif
        ...   $ echo shared
        ... end''', registertestcase=lambda _case: None))
        #if case1
        <BLANKLINE>
        if _testcase == 'case1':
        <BLANKLINE>
            checkoutput(sheval('echo case1\n'), '', src='$ echo case1\n', srcloc=2, outloc=3, endloc=3, indent=2, filename='')
        <BLANKLINE>
        #else
        else:
        <BLANKLINE>
            checkoutput(sheval('echo not case1\n'), '', src='$ echo not case1\n', srcloc=4, outloc=5, endloc=5, indent=2, filename='')
        <BLANKLINE>
        #endif
        #if case2
        <BLANKLINE>
        if _testcase == 'case2':
        <BLANKLINE>
            checkoutput(sheval('echo case2\n'), '', src='$ echo case2\n', srcloc=7, outloc=8, endloc=8, indent=2, filename='')
        <BLANKLINE>
        #endif
        <BLANKLINE>
        checkoutput(sheval('echo shared\n'), '', src='$ echo shared\n', srcloc=9, outloc=10, endloc=10, indent=2, filename='')
        <BLANKLINE>
        # end

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

    # "#if" support:
    # conditionstack is used for keeping a stack of "#if" results.
    # Each element it contains has a pair of bools that shows the result of
    # evaluating the last "#if" value on the first part of the value and the
    # result of "and"ing all the first values on the stack on the second one.
    # The meaning of these values is as follow:
    # - True: lines should be taken until #else or #endif
    # - False: lines should be ignored until #else or #endif
    # If it's empty, it means there are no "#if"s.
    conditionstack: List[Tuple[bool, bool]] = []

    def conditionstacktopeval() -> bool:
        nonlocal conditionstack
        return conditionstack[-1][1] if conditionstack else True

    extraindent = 0

    def appendline(line):
        if conditionstacktopeval():
            if extraindent:
                line = " " * (4 * extraindent) + line
            newlines.append(line)

    ifstack = []

    testcases = []

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
            # -4 spaces to match indented python code
            appendline(f"{' ' * max(info.indent - 4, 0)}{code}\n")
            nexti = j
        elif info.indent >= 4:
            # indented Python code
            maybeseparate("python")
            appendline(info.line[4:])
        elif info.line.startswith("#require "):
            maybeseparate("require")
            features = info.line[9:].split()
            if hasfeature is None:
                missing = features
            else:
                missing = [f for f in features if not hasfeature(f)]
            if missing:
                msg = f"missing feature: {' '.join(missing)}"
                appendline(f'raise __import__("unittest").SkipTest({repr(msg)})\n')
        elif info.line.startswith("#testcases "):
            assert registertestcase is not None
            newcases = info.line.split()[1:]
            for testcase in newcases:
                registertestcase(testcase)
            testcases.extend(newcases)
        elif info.line.startswith("#if "):
            maybeseparate("if")
            rawfeatures = info.line[4:].strip()
            features = rawfeatures.split()
            appendline(info.line)
            ifstack.append(rawfeatures)

            if rawfeatures in testcases:
                maybeseparate("python")
                appendline(f"if _testcase == '{rawfeatures}':\n")
                extraindent += 1
                condition = True
            elif hasfeature and all(hasfeature(f) for f in features):
                condition = True
            else:
                condition = False
            conditionstack.append((condition, condition and conditionstacktopeval()))
        elif info.line == "#else\n":
            maybeseparate("if")
            if ifstack[-1] in testcases:
                maybeseparate("python")
                extraindent -= 1
                appendline(info.line)
                appendline(f"else:\n")
                extraindent += 1
            elif conditionstack:
                condition = not conditionstack[-1][0]
                conditionstack.pop()
                conditionstack.append(
                    (condition, condition and conditionstacktopeval())
                )
                appendline(info.line)
        elif info.line == "#endif\n":
            maybeseparate("if")
            try:
                conditionstack.pop()
            except IndexError as e:
                raise e

            if ifstack.pop() in testcases:
                extraindent -= 1

            appendline(info.line)
        elif info.line.strip():
            assert (
                info.indent < 2
            ), f"invalid indentation at line {i} (2-space for $ or >>> blocks, 4-space for Python blocks): {info.line.strip()}"
            # Otherwise, it's a comment.
            maybeseparate("comment")
            appendline(f"# {info.line}")
        else:
            # Empty line
            appendline(info.line)
        i = nexti

    return "".join(newlines)
