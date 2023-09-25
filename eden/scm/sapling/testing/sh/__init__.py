# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

r"""simple shell interpreter

Interpret a subset of bash in Python to run .t tests.

Shell commands need to be explicitly defined. $PATH is
not respected.

By avoiding bash and ignoring $PATH, there are multiple benefits:
- External dependencies are more explicit.
- Much easier and faster to run on Windows.
- More flexible - can mix-in Python logic more easily.

Examples:

    >>> from .testfs import TestFS
    >>> def t(code):
    ...     env = Env(fs=TestFS(), cmdtable=dict(stdlib.cmdtable))
    ...     return sheval(code, env)

Simple command:

    >>> t('echo -n a  b')
    'a b (no-eol)\n'
    >>> t("printf '(%s)' 100")
    '(100) (no-eol)\n'
    >>> t('echo :')
    ':\n'

Expanding envvar:

    >>> t("A='B  C'; echo ${A}  '$A'  \"$A\"")
    'B C $A B  C\n'

Quoting:

    >>> t(r'''echo '"a"' ' ' '"b"' ''')
    '"a"   "b"\n'
    >>> t(r'''echo "'a'" " " "'"b"'" ''')
    "'a'   'b'\n"

Logical operation:

    >>> t("true && echo 1 || echo 2")
    '1\n'
    >>> t("false && echo 1 || echo 2")
    '2\n'
    >>> t("! true")
    '[1]\n'

Chain of commands:

    >>> t('echo x; echo y\n echo z')
    'x\ny\nz\n'
    >>> t('A=1; echo $A; echo $A')
    '1\n1\n'

Subshell, Compound:

    >>> t('{ echo 1; echo 2; }; (echo 3; echo 4; ); echo 5')
    '1\n2\n3\n4\n5\n'

Jobs:

    >>> t('echo 1 &\necho 2 &\nwait; wait')
    '1\n2\n'

Env Scope:

    >>> t('A=1; A=2 echo $A; echo $A')  # 'echo $A' resolved before A=2
    '1\n1\n'
    >>> t('A=1; A=2 env; env')  # A=2 exported for 1st env
    'A=2\n'
    >>> t('A=1; export A; env')
    'A=1\n'
    >>> t('export A=1; env')
    'A=1\n'
    >>> t('A=1; { A=2; }; echo $A')
    '2\n'
    >>> t('A=1; ( A=2; ); echo $A')
    '1\n'
    >>> t('A=1; f() { local A=2; echo f$A; }; f; echo $A')
    'f2\n1\n'
    >>> t('A=1; f() { local A; A=2; echo f$A; }; f; echo $A')
    'f2\n1\n'
    >>> t('A=1; f() { A=2; echo f$A; }; f; echo $A')
    'f2\n2\n'
    >>> t('export A=1; unset A; echo $A')
    '\n'

Math:

    >>> t('A=2; echo $((A+10))')
    '12\n'
    >>> t('echo $((2*3)) $((2**3)) $((4|2)) $((3&6)) $((5/2)) $((3<<1)) $((7>>2))')
    '6 8 6 2 2 6 1\n'
    >>> t('echo $((3*3-2*2+5)) $((1<2)) $((2==2)) $((1>=2))')
    '10 1 1 0\n'

If:

    >>> t('if true; then echo 1; else echo 2; fi')
    '1\n'
    >>> t('if false; then echo 1; else echo 2; fi')
    '2\n'
    >>> t('''if false && true; then
    ...          echo 1
    ...      elif false || true; then
    ...          echo 2
    ...      else
    ...          echo 3
    ...      fi''')
    '2\n'

For:

    >>> t('for i in 1 2 3 "4  5"; do echo $i; done')
    '1\n2\n3\n4 5\n'

While:

    >>> t('A=1; while [ $A -lt 3 ]; do echo $A; A=$((A+1)); done')
    '1\n2\n'
    >>> t('seq 3 | while read i; do echo $i; done')
    '1\n2\n3\n'

Case:

    >>> t('case g in\n f)\n true\n;;\n g)\n false\n;;\n esac')
    '[1]\n'

Redirect:

    >>> t('echo 1 >/dev/null')
    ''
    >>> t('echo 1 > a; echo 2 > a; echo 3 >> a; cat a; cat < a')
    '2\n3\n2\n3\n'
    >>> t('echo 1 > a; > a; cat a')
    ''
    >>> t('(echo 1 1>&2; ) 2>/dev/null')
    ''
    >>> t('A=1; ( A=2; echo 1 && echo 2; ) > a; cat a; echo $A')
    '1\n2\n1\n'
    >>> t('A=1; { A=2; echo 1 && echo $A; } > a; cat a; echo $A')
    '1\n2\n2\n'

Heredoc:

    >>> t('A=1; cat << EOF\n$A\n2\nEOF')
    '1\n2\n'
    >>> t("A=1; cat << 'EOF'\n$A\n2\nEOF")
    '$A\n2\n'

Function:

    >>> t('a() { echo "$@"; }; a "c  d"  e')
    'c  d e\n'
    >>> t('a() { echo $1 "$1" $3 ${5:-x} $#; }; a "p  q" r s t')
    'p q p  q s x 4\n'
    >>> t('f() { for i in "$@"; do echo _${i}_; done; }; f 1 2;')
    '_1_\n_2_\n'
    >>> t('f() { false; }; f')
    '[1]\n'
    >>> t('f() { A= echo 1; }; f')
    '1\n'

Pipe:

    >>> t('echo foo | cat | cat')
    'foo\n'
    >>> t('seq 10 | tail -5 | head -n3')
    '6\n7\n8\n'
    >>> t('seq 10 | ( read i; read j; echo $i $j )')
    '1 2\n'

Command substitution:

    >>> t('echo `echo foo` $(echo bar $(echo baz))')
    'foo bar baz\n'
    >>> t('''cat > a << EOF
    ... a
    ... $(echo b)
    ... c
    ... EOF
    ... cat a
    ... ''')
    'a\nb\nc\n'
    >>> t('A=1; echo `A=2; echo $A`; echo $A')
    '2\n1\n'

Substitution as arguments:

    >>> t(r"printf %s-%s \x y")
    'x-y (no-eol)\n'
    >>> t(r"printf %s-%s '\x y'")
    '\\x y- (no-eol)\n'
    >>> # note '\x' doesn't become 'x' with ``.
    >>> t(r"printf %s '\x y' > a; printf %s-%s `cat a`")
    '\\x-y (no-eol)\n'
    >>> t(r'''printf %s '\x y' > a; printf %s-%s "`cat a`" ''')
    '\\x y- (no-eol)\n'
    >>> t(r"A='C:\Users'; printf %s-%s $A $A/1")
    'C:\\Users-C:\\Users/1 (no-eol)\n'
    >>> t(r'''A='a\b'; B="b$A\c"; printf %s-%s "$A" "$B"''')
    'a\\b-ba\\b\\c (no-eol)\n'

Exit code ($?):

    >>> t('false; echo "$?"; echo $?')
    '1\n0\n'
    >>> t('( { false; }; ); echo "$?"; echo $?')
    '1\n0\n'

Glob:

    >>> t('touch a1 a2 b1 b2; echo a* c*')
    'a1 a2 c*\n'
    >>> t('touch a1 a2 a3; for i in a*; do echo $i; done')
    'a1\na2\na3\n'

Glob with substitution:

    >>> t('touch a1 a2 b1 b2; A=a; echo $A*')
    'a1 a2\n'

Tilde:

    >>> t('HOME=x; echo ~ a~ ~a ~/a')
    'x a~ ~a x/a\n'

Remove prefix:

    >>> t('A=abab; echo ${A##*a}')
    'b\n'

source:

    >>> t('''
    ... cat >> a.sh << 'EOF'
    ... foo() {
    ...   echo foo $@
    ... }
    ... A=2
    ... EOF
    ... source a.sh
    ... foo a b $A
    ... ''')
    'foo a b 2\n'

    >>> t('''
    ... cat >> a.sh << 'EOF'
    ... A=1
    ... B=2
    ... EOF
    ... source a.sh
    ... echo $A $B
    ... ''')
    '1 2\n'

    >>> t('''
    ... setenv() {
    ...   echo 'A=2' > a.sh
    ...   source a.sh
    ...   echo $A
    ... }
    ... setenv
    ... echo $A
    ... ''')
    '2\n2\n'

sh:

    >>> t("echo 'echo 1' > a.sh; sh a.sh")
    '1\n'
    >>> t("sh -c 'export A=1; echo INNER: $A'; echo OUTER: $A")
    'INNER: 1\nOUTER:\n'
    >>> t("echo 'echo [ $@ ]' > a.sh; sh a.sh 1 2")
    '[ 1 2 ]\n'
    >>> t("echo 'seq 10 | grep 9' > a.sh; sh a.sh")
    '9\n'

return:

    >>> t('{ echo 0; { echo 1; return 2; echo 3; }; echo 4; }')
    '0\n1\n[2]\n'
    >>> t('{ echo 0; ( echo 1; return 2; echo 3; ); echo 4; }')
    '0\n1\n4\n'
    >>> t('a() { echo 1; return 2; echo 3; }; a')
    '1\n[2]\n'
    >>> t('a() { echo 1; return 2; echo 3; }; a; echo 4')
    '1\n4\n'

exit:

    >>> t('{ echo 0; { echo 1; exit 2; echo 3; }; echo 4; }')
    '0\n1\n[2]\n'
    >>> t('{ echo 0; ( echo 1; exit 2; echo 3; ); echo 4; }')
    '0\n1\n4\n'
    >>> t('a() { echo 1; exit 2; echo 3; }; a; echo 4')
    '1\n[2]\n'

shift:

    >>> t('a() { echo $1 $#; shift; echo $1 $#; }; a 1 2 3')
    '1 3\n2 2\n'
    >>> t('a() { echo $1 $#; shift 2; echo $1 $#; }; a 1 2 3 4')
    '1 4\n3 2\n'

grep:

    >>> t('seq 20 | grep 2')
    '2\n12\n20\n'
    >>> t("seq 20 | grep '[12][05]'")
    '10\n15\n20\n'
    >>> t('seq 3 | grep -v 2')
    '1\n3\n'
    >>> t('echo a | grep b')
    '[1]\n'

sort

    >>> t('for i in c a b; do echo $i; done | sort')
    'a\nb\nc\n'

Commands on OS filesystem:

    >>> from .osfs import OSFS
    >>> import tempfile
    >>> def f(code):
    ...     with tempfile.TemporaryDirectory('.test-shinterp') as d:
    ...         fs = OSFS()
    ...         fs.chdir(d)
    ...         env = Env(fs=fs, cmdtable=dict(stdlib.cmdtable))
    ...         return sheval(code, env)

cp, rm, mv:

    >>> f('''
    ... mkdir a
    ... echo 1 > a/1
    ... cp a/1 a/2
    ... rm a/1
    ... mv a/2 a/3
    ... ls a
    ... ''')
    '3\n'

cd:

    >>> f('''
    ... mkdir -p a/b a/c
    ... cd a
    ... touch d
    ... ls
    ... cd ..
    ... ls
    ... ''')
    'b\nc\nd\na\n'

cp -R, rm -R:

    >>> f('''
    ... mkdir -p a/1 a/2
    ... cp -R a b
    ... rm -R a
    ... echo **/*
    ... ''')
    'b b/1 b/2\n'

tee:

    >>> f('echo a b | tee d e; cat d e')
    'a b\na b\na b\n'

test:

    >>> f('''
    ... mkdir a; touch a/1
    ... [ -d a ] && echo 'a is a dir'
    ... [ -f a/1 ] && echo 'a/1 is a file'
    ... ''')
    'a is a dir\na/1 is a file\n'

find:

    >>> print(f('''
    ... mkdir -p a/b/c d/b/z; touch a/b/d d/e
    ... echo find files:
    ... find . -type f
    ... echo find ../d:
    ... cd a; find ../d; cd ..
    ... echo find with patterns:
    ... find . -not -wholename '**/b/**' -type f
    ... ''').strip())
    find files:
    a/b/d
    d/e
    find ../d:
    ../d/b
    ../d/b/z
    ../d/e
    find with patterns:
    d/e

pwd == $PWD

    >>> f('[ "$(pwd)" = "$PWD" ] && echo pwd match')
    'pwd match\n'

wc -l:

    >>> t('seq 10 | wc -l')
    '10\n'
    >>> t('seq 10 > a; wc -l a')
    '10\n'

sed:

    >>> t("echo a a a c c c | sed -e 's#a#b' -e 's/c/d/g'")
    'b a a d d d\n'

    >>> t("seq 21 23 > a; sed -i 's#2#3' a; cat a")
    '31\n32\n33\n'
    >>> t('seq 3 | sed 2d')
    '1\n3\n'
    >>> t("seq 3 | sed '$d'")
    '1\n2\n'

py (python lookup):

    >>> pyval1 = 123
    >>> pyval2 = 'xyz'
    >>> t("py pyval1 not_found_name args pyval2")
    '123\nxyz\n'

"""

from . import stdlib
from .interp import sheval
from .types import Env, Scope

__all__ = ["stdlib", "sheval", "Env", "Scope"]
