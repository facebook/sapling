# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.autofix import eq
from testutil.dott.translate import translatebody


def translateeq(code, expected):
    eq(translatebody(code + "\n").strip(), expected, nested=1)


# comments
translateeq("foo", "# foo\n")
translateeq("# foo", "# foo\n")
translateeq(
    "A\n B\nC",
    r"""
    # A
    #  B
    # C
""",
)

# shell functions
translateeq("  $ echo 'foo bar' baz > a.txt", "sh % 'echo \"foo bar\" baz' > 'a.txt'")
translateeq(
    r"""
  $ cat p.txt < a.txt \
  > >> b.txt""",
    "sh % 'cat p.txt' << open('a.txt').read() >> 'b.txt'",
)
translateeq(
    r"""  $ true && false ; false""",
    r"""
    sh % 'true'
    sh % 'false'
    sh % 'false'""",
)

# testing output
translateeq(
    r"""
  $ echo foo bar
  foo bar
  $ cat a.txt
  a
  b
""",
    r'''
    sh % 'echo foo bar' == 'foo bar'
    sh % 'cat a.txt' == r"""
        a
        b"""''',
)

# heredoc as input
translateeq(
    r"""
  $ cat >> hgrc << EOF
  > [ui]
  > editor = foo
  > EOF
""",
    r'''
    sh % 'cat' << r"""
    [ui]
    editor = foo
    """ >> 'hgrc' ''',
)

# inline python
translateeq(
    r"""
  >>> for i in [1, 2]:
  ...     print(i)
""",
    r"""
    for i in [1, 2]:
        print(i)""",
)


# pipe
translateeq("  $ seq 1 10 | tail -2 >> b.txt", "sh % 'seq 1 10' | 'tail -2' >> 'b.txt'")


# FIXME: shell functions are not translated correctly
translateeq(
    r"""
  $ foo() {
  >   hg commit -m "$1"
  >   echo "$2"
  > }
""",
    r'''
    sh % '"foo()" "{"' == r"""
        >   hg commit -m "$1"
        >   echo "$2"
        > }"""''',
)

# FIXME: for loops are not translated correctly
translateeq(
    r"""
  $ for i in a b c; do
  >   hg commit -m $i
  > done
""",
    r'''
    sh % 'for i in a b "c;" do' == r"""
        >   hg commit -m $i
        > done"""''',
)

# '#if', '#require', '#testcases'
translateeq(
    r"""
#if symlink
  $ ln -s a b
#endif
  $ echo after
""",
    r"""
    if feature.check(['symlink']):
        sh % 'ln -s a b'

    sh % 'echo after'""",
)

translateeq(
    r"""
#require symlink execbit
""",
    "feature.require(['symlink', 'execbit'])",
)

translateeq(
    r"""
#testcases a b c
#if a
  $ echo a
#else
  $ echo b
#endif
""",
    r"""
    for testcase in ['a', 'b', 'c']:
        if feature.check(['a']):
            sh % 'echo a'
        else:
            sh % 'echo b'""",
)
