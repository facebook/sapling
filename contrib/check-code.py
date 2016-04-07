#!/usr/bin/env python
#
# check-code - a style and portability checker for Mercurial
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""style and portability checker for Mercurial

when a rule triggers wrong, do one of the following (prefer one from top):
 * do the work-around the rule suggests
 * doublecheck that it is a false match
 * improve the rule pattern
 * add an ignore pattern to the rule (3rd arg) which matches your good line
   (you can append a short comment and match this, like: #re-raises)
 * change the pattern to a warning and list the exception in test-check-code-hg
 * ONLY use no--check-code for skipping entire files from external sources
"""

from __future__ import absolute_import, print_function
import glob
import keyword
import optparse
import os
import re
import sys
try:
    import re2
except ImportError:
    re2 = None

def compilere(pat, multiline=False):
    if multiline:
        pat = '(?m)' + pat
    if re2:
        try:
            return re2.compile(pat)
        except re2.error:
            pass
    return re.compile(pat)

def repquote(m):
    fromc = '.:'
    tochr = 'pq'
    def encodechr(i):
        if i > 255:
            return 'u'
        c = chr(i)
        if c in ' \n':
            return c
        if c.isalpha():
            return 'x'
        if c.isdigit():
            return 'n'
        try:
            return tochr[fromc.find(c)]
        except (ValueError, IndexError):
            return 'o'
    t = m.group('text')
    tt = ''.join(encodechr(i) for i in xrange(256))
    t = t.translate(tt)
    return m.group('quote') + t + m.group('quote')

def reppython(m):
    comment = m.group('comment')
    if comment:
        l = len(comment.rstrip())
        return "#" * l + comment[l:]
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
  [
    (r'pushd|popd', "don't use 'pushd' or 'popd', use 'cd'"),
    (r'\W\$?\(\([^\)\n]*\)\)', "don't use (()) or $(()), use 'expr'"),
    (r'grep.*-q', "don't use 'grep -q', redirect to /dev/null"),
    (r'(?<!hg )grep.* -a', "don't use 'grep -a', use in-line python"),
    (r'sed.*-i', "don't use 'sed -i', use a temporary file"),
    (r'\becho\b.*\\n', "don't use 'echo \\n', use printf"),
    (r'echo -n', "don't use 'echo -n', use printf"),
    (r'(^|\|\s*)\bwc\b[^|]*$\n(?!.*\(re\))', "filter wc output"),
    (r'head -c', "don't use 'head -c', use 'dd'"),
    (r'tail -n', "don't use the '-n' option to tail, just use '-<num>'"),
    (r'sha1sum', "don't use sha1sum, use $TESTDIR/md5sum.py"),
    (r'ls.*-\w*R', "don't use 'ls -R', use 'find'"),
    (r'printf.*[^\\]\\([1-9]|0\d)', "don't use 'printf \NNN', use Python"),
    (r'printf.*[^\\]\\x', "don't use printf \\x, use Python"),
    (r'\$\(.*\)', "don't use $(expr), use `expr`"),
    (r'rm -rf \*', "don't use naked rm -rf, target a directory"),
    (r'(^|\|\s*)grep (-\w\s+)*[^|]*[(|]\w',
     "use egrep for extended grep syntax"),
    (r'/bin/', "don't use explicit paths for tools"),
    (r'[^\n]\Z', "no trailing newline"),
    (r'export .*=', "don't export and assign at once"),
    (r'^source\b', "don't use 'source', use '.'"),
    (r'touch -d', "don't use 'touch -d', use 'touch -t' instead"),
    (r'ls +[^|\n-]+ +-', "options to 'ls' must come before filenames"),
    (r'[^>\n]>\s*\$HGRCPATH', "don't overwrite $HGRCPATH, append to it"),
    (r'^stop\(\)', "don't use 'stop' as a shell function name"),
    (r'(\[|\btest\b).*-e ', "don't use 'test -e', use 'test -f'"),
    (r'\[\[\s+[^\]]*\]\]', "don't use '[[ ]]', use '[ ]'"),
    (r'^alias\b.*=', "don't use alias, use a function"),
    (r'if\s*!', "don't use '!' to negate exit status"),
    (r'/dev/u?random', "don't use entropy, use /dev/zero"),
    (r'do\s*true;\s*done', "don't use true as loop body, use sleep 0"),
    (r'^( *)\t', "don't use tabs to indent"),
    (r'sed (-e )?\'(\d+|/[^/]*/)i(?!\\\n)',
     "put a backslash-escaped newline after sed 'i' command"),
    (r'^diff *-\w*[uU].*$\n(^  \$ |^$)', "prefix diff -u/-U with cmp"),
    (r'^\s+(if)? diff *-\w*[uU]', "prefix diff -u/-U with cmp"),
    (r'seq ', "don't use 'seq', use $TESTDIR/seq.py"),
    (r'\butil\.Abort\b', "directly use error.Abort"),
    (r'\|&', "don't use |&, use 2>&1"),
    (r'\w =  +\w', "only one space after = allowed"),
    (r'\bsed\b.*[^\\]\\n', "don't use 'sed ... \\n', use a \\ and a newline"),
  ],
  # warnings
  [
    (r'^function', "don't use 'function', use old style"),
    (r'^diff.*-\w*N', "don't use 'diff -N'"),
    (r'\$PWD|\${PWD}', "don't use $PWD, use `pwd`"),
    (r'^([^"\'\n]|("[^"\n]*")|(\'[^\'\n]*\'))*\^', "^ must be quoted"),
    (r'kill (`|\$\()', "don't use kill, use killdaemons.py")
  ]
]

testfilters = [
    (r"( *)(#([^\n]*\S)?)", repcomment),
    (r"<<(\S+)((.|\n)*?\n\1)", rephere),
]

winglobmsg = "use (glob) to match Windows paths too"
uprefix = r"^  \$ "
utestpats = [
  [
    (r'^(\S.*||  [$>] \S.*)[ \t]\n', "trailing whitespace on non-output"),
    (uprefix + r'.*\|\s*sed[^|>\n]*\n',
     "use regex test output patterns instead of sed"),
    (uprefix + r'(true|exit 0)', "explicit zero exit unnecessary"),
    (uprefix + r'.*(?<!\[)\$\?', "explicit exit code checks unnecessary"),
    (uprefix + r'.*\|\| echo.*(fail|error)',
     "explicit exit code checks unnecessary"),
    (uprefix + r'set -e', "don't use set -e"),
    (uprefix + r'(\s|fi\b|done\b)', "use > for continued lines"),
    (uprefix + r'.*:\.\S*/', "x:.y in a path does not work on msys, rewrite "
     "as x://.y, or see `hg log -k msys` for alternatives", r'-\S+:\.|' #-Rxxx
     '# no-msys'), # in test-pull.t which is skipped on windows
    (r'^  saved backup bundle to \$TESTTMP.*\.hg$', winglobmsg),
    (r'^  changeset .* references (corrupted|missing) \$TESTTMP/.*[^)]$',
     winglobmsg),
    (r'^  pulling from \$TESTTMP/.*[^)]$', winglobmsg,
     '\$TESTTMP/unix-repo$'), # in test-issue1802.t which skipped on windows
    (r'^  reverting (?!subrepo ).*/.*[^)]$', winglobmsg),
    (r'^  cloning subrepo \S+/.*[^)]$', winglobmsg),
    (r'^  pushing to \$TESTTMP/.*[^)]$', winglobmsg),
    (r'^  pushing subrepo \S+/\S+ to.*[^)]$', winglobmsg),
    (r'^  moving \S+/.*[^)]$', winglobmsg),
    (r'^  no changes made to subrepo since.*/.*[^)]$', winglobmsg),
    (r'^  .*: largefile \S+ not available from file:.*/.*[^)]$', winglobmsg),
    (r'^  .*file://\$TESTTMP',
     'write "file:/*/$TESTTMP" + (glob) to match on windows too'),
    (r'^  (cat|find): .*: No such file or directory',
     'use test -f to test for file existence'),
    (r'^  diff -[^ -]*p',
     "don't use (external) diff with -p for portability"),
    (r'^  [-+][-+][-+] .* [-+]0000 \(glob\)',
     "glob timezone field in diff output for portability"),
    (r'^  @@ -[0-9]+ [+][0-9]+,[0-9]+ @@',
     "use '@@ -N* +N,n @@ (glob)' style chunk header for portability"),
    (r'^  @@ -[0-9]+,[0-9]+ [+][0-9]+ @@',
     "use '@@ -N,n +N* @@ (glob)' style chunk header for portability"),
    (r'^  @@ -[0-9]+ [+][0-9]+ @@',
     "use '@@ -N* +N* @@ (glob)' style chunk header for portability"),
    (uprefix + r'hg( +-[^ ]+( +[^ ]+)?)* +extdiff'
     r'( +(-[^ po-]+|--(?!program|option)[^ ]+|[^-][^ ]*))*$',
     "use $RUNTESTDIR/pdiff via extdiff (or -o/-p for false-positives)"),
  ],
  # warnings
  [
    (r'^  [^*?/\n]* \(glob\)$',
     "glob match with no glob character (?*/)"),
  ]
]

for i in [0, 1]:
    for tp in testpats[i]:
        p = tp[0]
        m = tp[1]
        if p.startswith(r'^'):
            p = r"^  [$>] (%s)" % p[1:]
        else:
            p = r"^  [$>] .*(%s)" % p
        utestpats[i].append((p, m) + tp[2:])

utestfilters = [
    (r"<<(\S+)((.|\n)*?\n  > \1)", rephere),
    (r"( *)(#([^\n]*\S)?)", repcomment),
]

pypats = [
  [
    (r'^\s*def\s*\w+\s*\(.*,\s*\(',
     "tuple parameter unpacking not available in Python 3+"),
    (r'lambda\s*\(.*,.*\)',
     "tuple parameter unpacking not available in Python 3+"),
    (r'(?<!def)\s+(cmp)\(', "cmp is not available in Python 3+"),
    (r'\breduce\s*\(.*', "reduce is not available in Python 3+"),
    (r'dict\(.*=', 'dict() is different in Py2 and 3 and is slower than {}',
     'dict-from-generator'),
    (r'\.has_key\b', "dict.has_key is not available in Python 3+"),
    (r'\s<>\s', '<> operator is not available in Python 3+, use !='),
    (r'^\s*\t', "don't use tabs"),
    (r'\S;\s*\n', "semicolon"),
    (r'[^_]_\([ \t\n]*(?:"[^"]+"[ \t\n+]*)+%', "don't use % inside _()"),
    (r"[^_]_\([ \t\n]*(?:'[^']+'[ \t\n+]*)+%", "don't use % inside _()"),
    (r'(\w|\)),\w', "missing whitespace after ,"),
    (r'(\w|\))[+/*\-<>]\w', "missing whitespace in expression"),
    (r'^\s+(\w|\.)+=\w[^,()\n]*$', "missing whitespace in assignment"),
    (r'\w\s=\s\s+\w', "gratuitous whitespace after ="),
    (r'.{81}', "line too long"),
    (r' x+[xo][\'"]\n\s+[\'"]x', 'string join across lines with no space'),
    (r'[^\n]\Z', "no trailing newline"),
    (r'(\S[ \t]+|^[ \t]+)\n', "trailing whitespace"),
#    (r'^\s+[^_ \n][^_. \n]+_[^_\n]+\s*=',
#     "don't use underbars in identifiers"),
    (r'^\s+(self\.)?[A-za-z][a-z0-9]+[A-Z]\w* = ',
     "don't use camelcase in identifiers"),
    (r'^\s*(if|while|def|class|except|try)\s[^[\n]*:\s*[^\\n]#\s]+',
     "linebreak after :"),
    (r'class\s[^( \n]+:', "old-style class, use class foo(object)",
     r'#.*old-style'),
    (r'class\s[^( \n]+\(\):',
     "class foo() creates old style object, use class foo(object)",
     r'#.*old-style'),
    (r'\b(%s)\(' % '|'.join(k for k in keyword.kwlist
                            if k not in ('print', 'exec')),
     "Python keyword is not a function"),
    (r',]', "unneeded trailing ',' in list"),
#    (r'class\s[A-Z][^\(]*\((?!Exception)',
#     "don't capitalize non-exception classes"),
#    (r'in range\(', "use xrange"),
#    (r'^\s*print\s+', "avoid using print in core and extensions"),
    (r'[\x80-\xff]', "non-ASCII character literal"),
    (r'("\')\.format\(', "str.format() has no bytes counterpart, use %"),
    (r'^\s*(%s)\s\s' % '|'.join(keyword.kwlist),
     "gratuitous whitespace after Python keyword"),
    (r'([\(\[][ \t]\S)|(\S[ \t][\)\]])', "gratuitous whitespace in () or []"),
#    (r'\s\s=', "gratuitous whitespace before ="),
    (r'[^>< ](\+=|-=|!=|<>|<=|>=|<<=|>>=|%=)\S',
     "missing whitespace around operator"),
    (r'[^>< ](\+=|-=|!=|<>|<=|>=|<<=|>>=|%=)\s',
     "missing whitespace around operator"),
    (r'\s(\+=|-=|!=|<>|<=|>=|<<=|>>=|%=)\S',
     "missing whitespace around operator"),
    (r'[^^+=*/!<>&| %-](\s=|=\s)[^= ]',
     "wrong whitespace around ="),
    (r'\([^()]*( =[^=]|[^<>!=]= )',
     "no whitespace around = for named parameters"),
    (r'raise Exception', "don't raise generic exceptions"),
    (r'raise [^,(]+, (\([^\)]+\)|[^,\(\)]+)$',
     "don't use old-style two-argument raise, use Exception(message)"),
    (r' is\s+(not\s+)?["\'0-9-]', "object comparison with literal"),
    (r' [=!]=\s+(True|False|None)',
     "comparison with singleton, use 'is' or 'is not' instead"),
    (r'^\s*(while|if) [01]:',
     "use True/False for constant Boolean expression"),
    (r'(?:(?<!def)\s+|\()hasattr',
     'hasattr(foo, bar) is broken, use util.safehasattr(foo, bar) instead'),
    (r'opener\([^)]*\).read\(',
     "use opener.read() instead"),
    (r'opener\([^)]*\).write\(',
     "use opener.write() instead"),
    (r'[\s\(](open|file)\([^)]*\)\.read\(',
     "use util.readfile() instead"),
    (r'[\s\(](open|file)\([^)]*\)\.write\(',
     "use util.writefile() instead"),
    (r'^[\s\(]*(open(er)?|file)\([^)]*\)',
     "always assign an opened file to a variable, and close it afterwards"),
    (r'[\s\(](open|file)\([^)]*\)\.',
     "always assign an opened file to a variable, and close it afterwards"),
    (r'(?i)descend[e]nt', "the proper spelling is descendAnt"),
    (r'\.debug\(\_', "don't mark debug messages for translation"),
    (r'\.strip\(\)\.split\(\)', "no need to strip before splitting"),
    (r'^\s*except\s*:', "naked except clause", r'#.*re-raises'),
    (r'^\s*except\s([^\(,]+|\([^\)]+\))\s*,',
     'legacy exception syntax; use "as" instead of ","'),
    (r':\n(    )*( ){1,3}[^ ]', "must indent 4 spaces"),
    (r'ui\.(status|progress|write|note|warn)\([\'\"]x',
     "missing _() in ui message (use () to hide false-positives)"),
    (r'release\(.*wlock, .*lock\)', "wrong lock release order"),
    (r'\b__bool__\b', "__bool__ should be __nonzero__ in Python 2"),
    (r'os\.path\.join\(.*, *(""|\'\')\)',
     "use pathutil.normasprefix(path) instead of os.path.join(path, '')"),
    (r'\s0[0-7]+\b', 'legacy octal syntax; use "0o" prefix instead of "0"'),
    # XXX only catch mutable arguments on the first line of the definition
    (r'def.*[( ]\w+=\{\}', "don't use mutable default arguments"),
    (r'\butil\.Abort\b', "directly use error.Abort"),
    (r'^import Queue', "don't use Queue, use util.queue + util.empty"),
    (r'^import cStringIO', "don't use cStringIO.StringIO, use util.stringio"),
    (r'^import urllib', "don't use urllib, use util.urlreq/util.urlerr"),
  ],
  # warnings
  [
    (r'(^| )pp +xxxxqq[ \n][^\n]', "add two newlines after '.. note::'"),
  ]
]

pyfilters = [
    (r"""(?msx)(?P<comment>\#.*?$)|
         ((?P<quote>('''|\"\"\"|(?<!')'(?!')|(?<!")"(?!")))
          (?P<text>(([^\\]|\\.)*?))
          (?P=quote))""", reppython),
]

txtfilters = []

txtpats = [
  [
    ('\s$', 'trailing whitespace'),
    ('.. note::[ \n][^\n]', 'add two newlines after note::')
  ],
  []
]

cpats = [
  [
    (r'//', "don't use //-style comments"),
    (r'^  ', "don't use spaces to indent"),
    (r'\S\t', "don't use tabs except for indent"),
    (r'(\S[ \t]+|^[ \t]+)\n', "trailing whitespace"),
    (r'.{81}', "line too long"),
    (r'(while|if|do|for)\(', "use space after while/if/do/for"),
    (r'return\(', "return is not a function"),
    (r' ;', "no space before ;"),
    (r'[^;] \)', "no space before )"),
    (r'[)][{]', "space between ) and {"),
    (r'\w+\* \w+', "use int *foo, not int* foo"),
    (r'\W\([^\)]+\) \w+', "use (int)foo, not (int) foo"),
    (r'\w+ (\+\+|--)', "use foo++, not foo ++"),
    (r'\w,\w', "missing whitespace after ,"),
    (r'^[^#]\w[+/*]\w', "missing whitespace in expression"),
    (r'\w\s=\s\s+\w', "gratuitous whitespace after ="),
    (r'^#\s+\w', "use #foo, not # foo"),
    (r'[^\n]\Z', "no trailing newline"),
    (r'^\s*#import\b', "use only #include in standard C code"),
    (r'strcpy\(', "don't use strcpy, use strlcpy or memcpy"),
    (r'strcat\(', "don't use strcat"),
  ],
  # warnings
  []
]

cfilters = [
    (r'(/\*)(((\*(?!/))|[^*])*)\*/', repccomment),
    (r'''(?P<quote>(?<!")")(?P<text>([^"]|\\")+)"(?!")''', repquote),
    (r'''(#\s*include\s+<)([^>]+)>''', repinclude),
    (r'(\()([^)]+\))', repcallspaces),
]

inutilpats = [
  [
    (r'\bui\.', "don't use ui in util"),
  ],
  # warnings
  []
]

inrevlogpats = [
  [
    (r'\brepo\.', "don't use repo in revlog"),
  ],
  # warnings
  []
]

webtemplatefilters = []

webtemplatepats = [
  [],
  [
    (r'{desc(\|(?!websub|firstline)[^\|]*)+}',
     'follow desc keyword with either firstline or websub'),
  ]
]

checks = [
    ('python', r'.*\.(py|cgi)$', r'^#!.*python', pyfilters, pypats),
    ('test script', r'(.*/)?test-[^.~]*$', '', testfilters, testpats),
    ('c', r'.*\.[ch]$', '', cfilters, cpats),
    ('unified test', r'.*\.t$', '', utestfilters, utestpats),
    ('layering violation repo in revlog', r'mercurial/revlog\.py', '',
     pyfilters, inrevlogpats),
    ('layering violation ui in util', r'mercurial/util\.py', '', pyfilters,
     inutilpats),
    ('txt', r'.*\.txt$', '', txtfilters, txtpats),
    ('web template', r'mercurial/templates/.*\.tmpl', '',
     webtemplatefilters, webtemplatepats),
]

def _preparepats():
    for c in checks:
        failandwarn = c[-1]
        for pats in failandwarn:
            for i, pseq in enumerate(pats):
                # fix-up regexes for multi-line searches
                p = pseq[0]
                # \s doesn't match \n
                p = re.sub(r'(?<!\\)\\s', r'[ \\t]', p)
                # [^...] doesn't match newline
                p = re.sub(r'(?<!\\)\[\^', r'[^\\n', p)

                pats[i] = (re.compile(p, re.MULTILINE),) + pseq[1:]
        filters = c[3]
        for i, flt in enumerate(filters):
            filters[i] = re.compile(flt[0]), flt[1]
_preparepats()

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
                print("%s:%d (%s):" % (fname, lineno, blame))
            else:
                print("%s:%d:" % (fname, lineno))
            print(" > %s" % line)
            self._lastseen = msgid
        print(" " + msg)

_defaultlogger = norepeatlogger()

def getblame(f):
    lines = []
    for l in os.popen('hg annotate -un %s' % f):
        start, line = l.split(':', 1)
        user, rev = start.split()
        lines.append((line[1:-1], user, rev))
    return lines

def checkfile(f, logfunc=_defaultlogger.log, maxerr=None, warnings=False,
              blame=False, debug=False, lineno=True):
    """checks style and portability of a given file

    :f: filepath
    :logfunc: function used to report error
              logfunc(filename, linenumber, linecontent, errormessage)
    :maxerr: number of error to display before aborting.
             Set to false (default) to report all errors

    return True if no error is found, False otherwise.
    """
    blamecache = None
    result = True

    try:
        fp = open(f)
    except IOError as e:
        print("Skipping %s, %s" % (f, str(e).split(':', 1)[0]))
        return result
    pre = post = fp.read()
    fp.close()

    for name, match, magic, filters, pats in checks:
        if debug:
            print(name, f)
        fc = 0
        if not (re.match(match, f) or (magic and re.search(magic, pre))):
            if debug:
                print("Skipping %s for %s it doesn't match %s" % (
                       name, match, f))
            continue
        if "no-" "check-code" in pre:
            # If you're looking at this line, it's because a file has:
            # no- check- code
            # but the reason to output skipping is to make life for
            # tests easier. So, instead of writing it with a normal
            # spelling, we write it with the expected spelling from
            # tests/test-check-code.t
            print("Skipping %s it has no-che?k-code (glob)" % f)
            return "Skip" # skip checking this file
        for p, r in filters:
            post = re.sub(p, r, post)
        nerrs = len(pats[0]) # nerr elements are errors
        if warnings:
            pats = pats[0] + pats[1]
        else:
            pats = pats[0]
        # print post # uncomment to show filtered version

        if debug:
            print("Checking %s for %s" % (name, f))

        prelines = None
        errors = []
        for i, pat in enumerate(pats):
            if len(pat) == 3:
                p, msg, ignore = pat
            else:
                p, msg = pat
                ignore = None
            if i >= nerrs:
                msg = "warning: " + msg

            pos = 0
            n = 0
            for m in p.finditer(post):
                if prelines is None:
                    prelines = pre.splitlines()
                    postlines = post.splitlines(True)

                start = m.start()
                while n < len(postlines):
                    step = len(postlines[n])
                    if pos + step > start:
                        break
                    pos += step
                    n += 1
                l = prelines[n]

                if ignore and re.search(ignore, l, re.MULTILINE):
                    if debug:
                        print("Skipping %s for %s:%s (ignore pattern)" % (
                            name, f, n))
                    continue
                bd = ""
                if blame:
                    bd = 'working directory'
                    if not blamecache:
                        blamecache = getblame(f)
                    if n < len(blamecache):
                        bl, bu, br = blamecache[n]
                        if bl == l:
                            bd = '%s@%s' % (bu, br)

                errors.append((f, lineno and n + 1, l, msg, bd))
                result = False

        errors.sort()
        for e in errors:
            logfunc(*e)
            fc += 1
            if maxerr and fc >= maxerr:
                print(" (too many errors, giving up)")
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
    parser.add_option("", "--debug", action="store_true",
                      help="show debug information")
    parser.add_option("", "--nolineno", action="store_false",
                      dest='lineno', help="don't show line numbers")

    parser.set_defaults(per_file=15, warnings=False, blame=False, debug=False,
                        lineno=True)
    (options, args) = parser.parse_args()

    if len(args) == 0:
        check = glob.glob("*")
    else:
        check = args

    ret = 0
    for f in check:
        if not checkfile(f, maxerr=options.per_file, warnings=options.warnings,
                         blame=options.blame, debug=options.debug,
                         lineno=options.lineno):
            ret = 1
    sys.exit(ret)
