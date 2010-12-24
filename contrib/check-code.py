#!/usr/bin/env python
#
# check-code - a style and portability checker for Mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import re, glob, os, sys
import keyword
import optparse

def repquote(m):
    t = re.sub(r"\w", "x", m.group('text'))
    t = re.sub(r"[^\sx]", "o", t)
    return m.group('quote') + t + m.group('quote')

def reppython(m):
    comment = m.group('comment')
    if comment:
        return "#" * len(comment)
    return repquote(m)

def repcomment(m):
    return m.group(1) + "#" * len(m.group(2))

def repccomment(m):
    t = re.sub(r"((?<=\n) )|\S", "x", m.group(2))
    return m.group(1) + t + "*/"

def repcallspaces(m):
    t = re.sub(r"\n\s+", "\n", m.group(2))
    return m.group(1) + t

def repinclude(m):
    return m.group(1) + "<foo>"

def rephere(m):
    t = re.sub(r"\S", "x", m.group(2))
    return m.group(1) + t


testpats = [
    (r'(pushd|popd)', "don't use 'pushd' or 'popd', use 'cd'"),
    (r'\W\$?\(\([^\)]*\)\)', "don't use (()) or $(()), use 'expr'"),
    (r'^function', "don't use 'function', use old style"),
    (r'grep.*-q', "don't use 'grep -q', redirect to /dev/null"),
    (r'echo.*\\n', "don't use 'echo \\n', use printf"),
    (r'echo -n', "don't use 'echo -n', use printf"),
    (r'^diff.*-\w*N', "don't use 'diff -N'"),
    (r'(^| )wc[^|]*$', "filter wc output"),
    (r'head -c', "don't use 'head -c', use 'dd'"),
    (r'ls.*-\w*R', "don't use 'ls -R', use 'find'"),
    (r'printf.*\\\d\d\d', "don't use 'printf \NNN', use Python"),
    (r'printf.*\\x', "don't use printf \\x, use Python"),
    (r'\$\(.*\)', "don't use $(expr), use `expr`"),
    (r'rm -rf \*', "don't use naked rm -rf, target a directory"),
    (r'(^|\|\s*)grep (-\w\s+)*[^|]*[(|]\w',
     "use egrep for extended grep syntax"),
    (r'/bin/', "don't use explicit paths for tools"),
    (r'\$PWD', "don't use $PWD, use `pwd`"),
    (r'[^\n]\Z', "no trailing newline"),
    (r'export.*=', "don't export and assign at once"),
    ('^([^"\']|("[^"]*")|(\'[^\']*\'))*\\^', "^ must be quoted"),
    (r'^source\b', "don't use 'source', use '.'"),
    (r'touch -d', "don't use 'touch -d', use 'touch -t' instead"),
    (r'ls\s+[^|-]+\s+-', "options to 'ls' must come before filenames"),
]

testfilters = [
    (r"( *)(#([^\n]*\S)?)", repcomment),
    (r"<<(\S+)((.|\n)*?\n\1)", rephere),
]

uprefix = r"^  \$ "
uprefixc = r"^  > "
utestpats = [
    (r'^(\S|  $ ).*(\S\s+|^\s+)\n', "trailing whitespace on non-output"),
    (uprefix + r'.*\|\s*sed', "use regex test output patterns instead of sed"),
    (uprefix + r'(true|exit 0)', "explicit zero exit unnecessary"),
    (uprefix + r'.*\$\?', "explicit exit code checks unnecessary"),
    (uprefix + r'.*\|\| echo.*(fail|error)',
     "explicit exit code checks unnecessary"),
    (uprefix + r'set -e', "don't use set -e"),
    (uprefixc + r'( *)\t', "don't use tabs to indent"),
]

for p, m in testpats:
    if p.startswith('^'):
        p = uprefix + p[1:]
    else:
        p = uprefix + p
    utestpats.append((p, m))

utestfilters = [
    (r"( *)(#([^\n]*\S)?)", repcomment),
]

pypats = [
    (r'^\s*def\s*\w+\s*\(.*,\s*\(',
     "tuple parameter unpacking not available in Python 3+"),
    (r'lambda\s*\(.*,.*\)',
     "tuple parameter unpacking not available in Python 3+"),
    (r'(?<!def)\s+(cmp)\(', "cmp is not available in Python 3+"),
    (r'\breduce\s*\(.*', "reduce is not available in Python 3+"),
    (r'\.has_key\b', "dict.has_key is not available in Python 3+"),
    (r'^\s*\t', "don't use tabs"),
    (r'\S;\s*\n', "semicolon"),
    (r'\w,\w', "missing whitespace after ,"),
    (r'\w[+/*\-<>]\w', "missing whitespace in expression"),
    (r'^\s+\w+=\w+[^,)]$', "missing whitespace in assignment"),
    (r'.{85}', "line too long"),
    (r'.{81}', "warning: line over 80 characters"),
    (r'[^\n]\Z', "no trailing newline"),
    (r'(\S\s+|^\s+)\n', "trailing whitespace"),
#    (r'^\s+[^_ ][^_. ]+_[^_]+\s*=', "don't use underbars in identifiers"),
#    (r'\w*[a-z][A-Z]\w*\s*=', "don't use camelcase in identifiers"),
    (r'^\s*(if|while|def|class|except|try)\s[^[]*:\s*[^\]#\s]+',
     "linebreak after :"),
    (r'class\s[^(]:', "old-style class, use class foo(object)"),
    (r'\b(%s)\(' % '|'.join(keyword.kwlist),
     "Python keyword is not a function"),
    (r',]', "unneeded trailing ',' in list"),
#    (r'class\s[A-Z][^\(]*\((?!Exception)',
#     "don't capitalize non-exception classes"),
#    (r'in range\(', "use xrange"),
#    (r'^\s*print\s+', "avoid using print in core and extensions"),
    (r'[\x80-\xff]', "non-ASCII character literal"),
    (r'("\')\.format\(', "str.format() not available in Python 2.4"),
    (r'^\s*with\s+', "with not available in Python 2.4"),
    (r'^\s*except.* as .*:', "except as not available in Python 2.4"),
    (r'^\s*os\.path\.relpath', "relpath not available in Python 2.4"),
    (r'(?<!def)\s+(any|all|format)\(',
     "any/all/format not available in Python 2.4"),
    (r'(?<!def)\s+(callable)\(',
     "callable not available in Python 3, use hasattr(f, '__call__')"),
    (r'if\s.*\selse', "if ... else form not available in Python 2.4"),
    (r'^\s*(%s)\s\s' % '|'.join(keyword.kwlist),
     "gratuitous whitespace after Python keyword"),
    (r'([\(\[]\s\S)|(\S\s[\)\]])', "gratuitous whitespace in () or []"),
#    (r'\s\s=', "gratuitous whitespace before ="),
    (r'[^>< ](\+=|-=|!=|<>|<=|>=|<<=|>>=)\S',
     "missing whitespace around operator"),
    (r'[^>< ](\+=|-=|!=|<>|<=|>=|<<=|>>=)\s',
     "missing whitespace around operator"),
    (r'\s(\+=|-=|!=|<>|<=|>=|<<=|>>=)\S',
     "missing whitespace around operator"),
    (r'[^+=*!<>&| -](\s=|=\s)[^= ]',
     "wrong whitespace around ="),
    (r'raise Exception', "don't raise generic exceptions"),
    (r'ui\.(status|progress|write|note|warn)\([\'\"]x',
     "warning: unwrapped ui message"),
    (r' is\s+(not\s+)?["\'0-9-]', "object comparison with literal"),
    (r' [=!]=\s+(True|False|None)',
     "comparison with singleton, use 'is' or 'is not' instead"),
]

pyfilters = [
    (r"""(?msx)(?P<comment>\#.*?$)|
         ((?P<quote>('''|\"\"\"|(?<!')'(?!')|(?<!")"(?!")))
          (?P<text>(([^\\]|\\.)*?))
          (?P=quote))""", reppython),
]

cpats = [
    (r'//', "don't use //-style comments"),
    (r'^  ', "don't use spaces to indent"),
    (r'\S\t', "don't use tabs except for indent"),
    (r'(\S\s+|^\s+)\n', "trailing whitespace"),
    (r'.{85}', "line too long"),
    (r'(while|if|do|for)\(', "use space after while/if/do/for"),
    (r'return\(', "return is not a function"),
    (r' ;', "no space before ;"),
    (r'\w+\* \w+', "use int *foo, not int* foo"),
    (r'\([^\)]+\) \w+', "use (int)foo, not (int) foo"),
    (r'\S+ (\+\+|--)', "use foo++, not foo ++"),
    (r'\w,\w', "missing whitespace after ,"),
    (r'\w[+/*]\w', "missing whitespace in expression"),
    (r'^#\s+\w', "use #foo, not # foo"),
    (r'[^\n]\Z', "no trailing newline"),
]

cfilters = [
    (r'(/\*)(((\*(?!/))|[^*])*)\*/', repccomment),
    (r'''(?P<quote>(?<!")")(?P<text>([^"]|\\")+)"(?!")''', repquote),
    (r'''(#\s*include\s+<)([^>]+)>''', repinclude),
    (r'(\()([^)]+\))', repcallspaces),
]

checks = [
    ('python', r'.*\.(py|cgi)$', pyfilters, pypats),
    ('test script', r'(.*/)?test-[^.~]*$', testfilters, testpats),
    ('c', r'.*\.c$', cfilters, cpats),
    ('unified test', r'.*\.t$', utestfilters, utestpats),
]

class norepeatlogger(object):
    def __init__(self):
        self._lastseen = None

    def log(self, fname, lineno, line, msg, blame):
        """print error related a to given line of a given file.

        The faulty line will also be printed but only once in the case
        of multiple errors.

        :fname: filename
        :lineno: line number
        :line: actual content of the line
        :msg: error message
        """
        msgid = fname, lineno, line
        if msgid != self._lastseen:
            if blame:
                print "%s:%d (%s):" % (fname, lineno, blame)
            else:
                print "%s:%d:" % (fname, lineno)
            print " > %s" % line
            self._lastseen = msgid
        print " " + msg

_defaultlogger = norepeatlogger()

def getblame(f):
    lines = []
    for l in os.popen('hg annotate -un %s' % f):
        start, line = l.split(':', 1)
        user, rev = start.split()
        lines.append((line[1:-1], user, rev))
    return lines

def checkfile(f, logfunc=_defaultlogger.log, maxerr=None, warnings=False,
              blame=False):
    """checks style and portability of a given file

    :f: filepath
    :logfunc: function used to report error
              logfunc(filename, linenumber, linecontent, errormessage)
    :maxerr: number of error to display before arborting.
             Set to None (default) to report all errors

    return True if no error is found, False otherwise.
    """
    blamecache = None
    result = True
    for name, match, filters, pats in checks:
        fc = 0
        if not re.match(match, f):
            continue
        fp = open(f)
        pre = post = fp.read()
        fp.close()
        if "no-" + "check-code" in pre:
            break
        for p, r in filters:
            post = re.sub(p, r, post)
        # print post # uncomment to show filtered version
        z = enumerate(zip(pre.splitlines(), post.splitlines(True)))
        for n, l in z:
            if "check-code" + "-ignore" in l[0]:
                continue
            for p, msg in pats:
                if not warnings and msg.startswith("warning"):
                    continue
                if re.search(p, l[1]):
                    bd = ""
                    if blame:
                        bd = 'working directory'
                        if not blamecache:
                            blamecache = getblame(f)
                        if n < len(blamecache):
                            bl, bu, br = blamecache[n]
                            if bl == l[0]:
                                bd = '%s@%s' % (bu, br)
                    logfunc(f, n + 1, l[0], msg, bd)
                    fc += 1
                    result = False
            if maxerr is not None and fc >= maxerr:
                print " (too many errors, giving up)"
                break
        break
    return result

if __name__ == "__main__":
    parser = optparse.OptionParser("%prog [options] [files]")
    parser.add_option("-w", "--warnings", action="store_true",
                      help="include warning-level checks")
    parser.add_option("-p", "--per-file", type="int",
                      help="max warnings per file")
    parser.add_option("-b", "--blame", action="store_true",
                      help="use annotate to generate blame info")

    parser.set_defaults(per_file=15, warnings=False, blame=False)
    (options, args) = parser.parse_args()

    if len(args) == 0:
        check = glob.glob("*")
    else:
        check = args

    for f in check:
        ret = 0
        if not checkfile(f, maxerr=options.per_file, warnings=options.warnings,
                         blame=options.blame):
            ret = 1
    sys.exit(ret)
