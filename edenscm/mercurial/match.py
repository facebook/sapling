# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# match.py - filename matching
#
#  Copyright 2008, 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import copy
import os
import re

from bindings import pathmatcher

from . import error, pathutil, util
from .i18n import _


allpatternkinds = (
    "re",
    "glob",
    "path",
    "relglob",
    "relpath",
    "relre",
    "listfile",
    "listfile0",
    "set",
    "include",
    "subinclude",
    "rootfilesin",
)
cwdrelativepatternkinds = ("relpath", "glob")

propertycache = util.propertycache


def _rematcher(regex):
    """compile the regexp with the best available regexp engine and return a
    matcher function"""
    m = util.re.compile(regex)
    try:
        # slightly faster, provided by facebook's re2 bindings
        return m.test_match
    except AttributeError:
        return m.match


def _expandsets(kindpats, ctx):
    """Returns the kindpats list with the 'set' patterns expanded."""
    fset = set()
    other = []

    for kind, pat, source in kindpats:
        if kind == "set":
            if not ctx:
                raise error.ProgrammingError("fileset expression with no " "context")
            s = ctx.getfileset(pat)
            fset.update(s)
            continue
        other.append((kind, pat, source))
    return fset, other


def _expandsubinclude(kindpats, root):
    """Returns the list of subinclude matcher args and the kindpats without the
    subincludes in it."""
    relmatchers = []
    other = []

    for kind, pat, source in kindpats:
        if kind == "subinclude":
            sourceroot = pathutil.dirname(util.normpath(source))
            pat = util.pconvert(pat)
            path = pathutil.join(sourceroot, pat)

            newroot = pathutil.dirname(path)
            matcherargs = (newroot, "", [], ["include:%s" % path])

            prefix = pathutil.canonpath(root, root, newroot)
            if prefix:
                prefix += "/"
            relmatchers.append((prefix, matcherargs))
        else:
            other.append((kind, pat, source))

    return relmatchers, other


def _kindpatsalwaysmatch(kindpats):
    """"Checks whether the kindspats match everything, as e.g.
    'relpath:.' does.
    """
    for kind, pat, source in kindpats:
        # TODO: update me?
        if pat != "" or kind not in ["relpath", "glob"]:
            return False
    return True


def match(
    root,
    cwd,
    patterns=None,
    include=None,
    exclude=None,
    default="glob",
    exact=False,
    auditor=None,
    ctx=None,
    warn=None,
    badfn=None,
    icasefs=False,
):
    """build an object to match a set of file patterns

    arguments:
    root - the canonical root of the tree you're matching against
    cwd - the current working directory, if relevant
    patterns - patterns to find
    include - patterns to include (unless they are excluded)
    exclude - patterns to exclude (even if they are included)
    default - if a pattern in patterns has no explicit type, assume this one
    exact - patterns are actually filenames (include/exclude still apply)
    warn - optional function used for printing warnings
    badfn - optional bad() callback for this matcher instead of the default
    icasefs - make a matcher for wdir on case insensitive filesystems, which
        normalizes the given patterns to the case in the filesystem

    a pattern is one of:
    'glob:<glob>' - a glob relative to cwd
    're:<regexp>' - a regular expression
    'path:<path>' - a path relative to repository root, which is matched
                    recursively
    'rootfilesin:<path>' - a path relative to repository root, which is
                    matched non-recursively (will not match subdirectories)
    'relglob:<glob>' - an unrooted glob (*.c matches C files in all dirs)
    'relpath:<path>' - a path relative to cwd
    'relre:<regexp>' - a regexp that needn't match the start of a name
    'set:<fileset>' - a fileset expression
    'include:<path>' - a file of patterns to read and include
    'subinclude:<path>' - a file of patterns to match against files under
                          the same directory
    '<something>' - a pattern of the specified default type
    """
    if auditor is None:
        auditor = pathutil.pathauditor(root)
    normalize = _donormalize
    if icasefs:
        if exact:
            raise error.ProgrammingError(
                "a case-insensitive exact matcher " "doesn't make sense"
            )
        dirstate = ctx.repo().dirstate
        dsnormalize = dirstate.normalize

        def normalize(patterns, default, root, cwd, auditor, warn):
            kp = _donormalize(patterns, default, root, cwd, auditor, warn)
            kindpats = []
            for kind, pats, source in kp:
                if kind not in ("re", "relre"):  # regex can't be normalized
                    p = pats
                    pats = dsnormalize(pats)

                    # Preserve the original to handle a case only rename.
                    if p != pats and p in dirstate:
                        kindpats.append((kind, p, source))

                kindpats.append((kind, pats, source))
            return kindpats

    if exact:
        m = exactmatcher(root, cwd, patterns, badfn)
    elif patterns:
        kindpats = normalize(patterns, default, root, cwd, auditor, warn)
        if _kindpatsalwaysmatch(kindpats):
            m = alwaysmatcher(root, cwd, badfn, relativeuipath=True)
        else:
            m = patternmatcher(root, cwd, kindpats, ctx=ctx, badfn=badfn)
    else:
        # It's a little strange that no patterns means to match everything.
        # Consider changing this to match nothing (probably using nevermatcher).
        m = alwaysmatcher(root, cwd, badfn)

    if include:
        kindpats = normalize(include, "glob", root, cwd, auditor, warn)
        im = includematcher(root, cwd, kindpats, ctx=ctx, badfn=None)
        m = intersectmatchers(m, im)
    if exclude:
        kindpats = normalize(exclude, "glob", root, cwd, auditor, warn)
        em = includematcher(root, cwd, kindpats, ctx=ctx, badfn=None)
        m = differencematcher(m, em)
    return m


def exact(root, cwd, files, badfn=None):
    return exactmatcher(root, cwd, files, badfn=badfn)


def always(root, cwd):
    return alwaysmatcher(root, cwd)


def never(root, cwd):
    return nevermatcher(root, cwd)


def union(matches, root, cwd):
    """Union a list of matchers.

    If the list is empty, return nevermatcher.
    If the list only contains one non-None value, return that matcher.
    Otherwise return a union matcher.
    """
    matches = filter(None, matches)
    if len(matches) == 0:
        return nevermatcher(root, cwd)
    elif len(matches) == 1:
        return matches[0]
    else:
        return unionmatcher(matches)


def badmatch(match, badfn):
    """Make a copy of the given matcher, replacing its bad method with the given
    one.
    """
    m = copy.copy(match)
    m.bad = badfn
    return m


def _donormalize(patterns, default, root, cwd, auditor, warn):
    """Convert 'kind:pat' from the patterns list to tuples with kind and
    normalized and rooted patterns and with listfiles expanded."""
    kindpats = []
    for kind, pat in [_patsplit(p, default) for p in patterns]:
        if kind in cwdrelativepatternkinds:
            pat = pathutil.canonpath(root, cwd, pat, auditor)
        elif kind in ("relglob", "path", "rootfilesin"):
            pat = util.normpath(pat)
        elif kind in ("listfile", "listfile0"):
            try:
                files = util.readfile(pat)
                if kind == "listfile0":
                    files = files.split("\0")
                else:
                    files = files.splitlines()
                files = [f for f in files if f]
            except EnvironmentError:
                raise error.Abort(_("unable to read file list (%s)") % pat)
            for k, p, source in _donormalize(files, default, root, cwd, auditor, warn):
                kindpats.append((k, p, pat))
            continue
        elif kind == "include":
            try:
                fullpath = os.path.join(root, util.localpath(pat))
                includepats = readpatternfile(fullpath, warn)
                for k, p, source in _donormalize(
                    includepats, default, root, cwd, auditor, warn
                ):
                    kindpats.append((k, p, source or pat))
            except error.Abort as inst:
                raise error.Abort("%s: %s" % (pat, inst[0]))
            except IOError as inst:
                if warn:
                    warn(
                        _("skipping unreadable pattern file '%s': %s\n")
                        % (pat, inst.strerror)
                    )
            continue
        # else: re or relre - which cannot be normalized
        kindpats.append((kind, pat, ""))
    return kindpats


def _testrefastpath(repat):
    """Test if a re pattern can use fast path.

    That is, for every "$A/$B" path the pattern matches, "$A" must also be
    matched,

    Return True if we're sure it is. Return False otherwise.
    """
    # XXX: It's very hard to implement this. These are what need to be
    # supported in production and tests. Very hacky. But we plan to get rid
    # of re matchers eventually.

    # Rules like "(?!experimental/)"
    if repat.startswith("(?!") and repat.endswith(")") and repat.count(")") == 1:
        return True

    # Rules used in doctest
    if repat == "(i|j)$":
        return True

    return False


def _globpatsplit(pat):
    """Split a glob pattern. Return a list.

    A naive version is "path.split("/")". This function handles more cases, like
    "{*,{a,b}*/*}".

    >>> _globpatsplit("*/**/x/{a,b/c}")
    ['*', '**', 'x', '{a,b/c}']
    """
    result = []
    buf = ""
    parentheses = 0
    for ch in pat:
        if ch == "{":
            parentheses += 1
        elif ch == "}":
            parentheses -= 1
        if parentheses == 0 and ch == "/":
            if buf:
                result.append(buf)
                buf = ""
        else:
            buf += ch
    if buf:
        result.append(buf)
    return result


class _tree(dict):
    """A tree intended to answer "visitdir" questions with more efficient
    answers (ex. return "all" or False if possible).
    """

    def __init__(self, *args, **kwargs):
        # If True, avoid entering subdirectories, and match everything recursively,
        # unconditionally.
        self.matchrecursive = False
        # If True, avoid entering subdirectories, and return "unsure" for
        # everything. This is set to True when complex re patterns (potentially
        # including "/") are used.
        self.unsurerecursive = False
        # Patterns for matching paths in this directory.
        self._kindpats = []
        # Glob patterns used to match parent directories of another glob
        # pattern.
        self._globdirpats = []
        super(_tree, self).__init__(*args, **kwargs)

    def insert(self, path, matchrecursive=True, globpats=None, repats=None):
        """Insert a directory path to this tree.

        If matchrecursive is True, mark the directory as unconditionally
        include files and subdirs recursively.

        If globpats or repats are specified, append them to the patterns being
        applied at this directory. The tricky part is those patterns can match
        "x/y/z" and visit("x"), visit("x/y") need to return True, while we
        still want visit("x/a") to return False.
        """
        if path == "":
            self.matchrecursive |= matchrecursive
            if globpats:
                # Need to match parent directories too.
                for pat in globpats:
                    components = _globpatsplit(pat)
                    parentpat = ""
                    for comp in components:
                        if parentpat:
                            parentpat += "/"
                        parentpat += comp
                        if "/" in comp:
                            # Giving up - fallback to slow paths.
                            self.unsurerecursive = True
                        self._globdirpats.append(parentpat)
                if any("**" in p for p in globpats):
                    # Giving up - "**" matches paths including "/"
                    self.unsurerecursive = True
                self._kindpats += [("glob", pat, "") for pat in globpats]
            if repats:
                if not all(map(_testrefastpath, repats)):
                    # Giving up - fallback to slow paths.
                    self.unsurerecursive = True
                self._kindpats += [("re", pat, "") for pat in repats]
            return

        subdir, rest = self._split(path)
        self.setdefault(subdir, _tree()).insert(rest, matchrecursive, globpats, repats)

    def visitdir(self, path):
        """Similar to matcher.visitdir"""
        path = normalizerootdir(path, "visitdir")
        if self.matchrecursive:
            return "all"
        elif self.unsurerecursive:
            return True
        elif path == "":
            return True

        if self._kindpats and self._compiledpats(path):
            # XXX: This is incorrect. But re patterns are already used in
            # production. We should kill them!
            # Need to test "if every string starting with 'path' matches".
            # Obviously it's impossible to test *every* string with the
            # standard regex API, therefore pick a random strange path to test
            # it approximately.
            if self._compiledpats("%s/*/_/-/0/*" % path):
                return "all"
            else:
                return True

        if self._globdirpats and self._compileddirpats(path):
            return True

        subdir, rest = self._split(path)
        subtree = self.get(subdir)
        if subtree is None:
            return False
        else:
            return subtree.visitdir(rest)

    @util.propertycache
    def _compiledpats(self):
        pat, matchfunc = _buildregexmatch(self._kindpats, "")
        return matchfunc

    @util.propertycache
    def _compileddirpats(self):
        pat, matchfunc = _buildregexmatch(
            [("glob", p, "") for p in self._globdirpats], "$"
        )
        return matchfunc

    def _split(self, path):
        if "/" in path:
            subdir, rest = path.split("/", 1)
        else:
            subdir, rest = path, ""
        if not subdir:
            raise error.ProgrammingError("path cannot be absolute")
        return subdir, rest


def _remainingpats(pat, prefix):
    """list of patterns with prefix stripped

    >>> _remainingpats("a/b/c", "")
    ['a/b/c']
    >>> _remainingpats("a/b/c", "a")
    ['b/c']
    >>> _remainingpats("a/b/c", "a/b")
    ['c']
    >>> _remainingpats("a/b/c", "a/b/c")
    []
    >>> _remainingpats("", "")
    []
    """
    if prefix:
        if prefix == pat:
            return []
        else:
            assert pat[len(prefix)] == "/"
            return [pat[len(prefix) + 1 :]]
    else:
        if pat:
            return [pat]
        else:
            return []


def _buildvisitdir(kindpats):
    """Try to build an efficient visitdir function

    Return a visitdir function if it's built. Otherwise return None
    if there are unsupported patterns.

    >>> _buildvisitdir([('include', 'foo', '')])
    >>> _buildvisitdir([('relglob', 'foo', '')])
    >>> t = _buildvisitdir([
    ...     ('glob', 'a/b', ''),
    ...     ('glob', 'c/*.d', ''),
    ...     ('glob', 'e/**/*.c', ''),
    ...     ('re', '^f/(?!g)', ''), # no "$", only match prefix
    ...     ('re', '^h/(i|j)$', ''),
    ...     ('glob', 'i/a*/b*/c*', ''),
    ...     ('glob', 'i/a5/b7/d', ''),
    ...     ('glob', 'j/**.c', ''),
    ... ])
    >>> t('a')
    True
    >>> t('a/b')
    'all'
    >>> t('a/b/c')
    'all'
    >>> t('c')
    True
    >>> t('c/d')
    False
    >>> t('c/rc.d')
    'all'
    >>> t('c/rc.d/foo')
    'all'
    >>> t('e')
    True
    >>> t('e/a')
    True
    >>> t('e/a/b.c')
    True
    >>> t('e/a/b.d')
    True
    >>> t('f')
    True
    >>> t('f/g')
    False
    >>> t('f/g2')
    False
    >>> t('f/g/a')
    False
    >>> t('f/h')
    'all'
    >>> t('f/h/i')
    'all'
    >>> t('h/i')
    True
    >>> t('h/i/k')
    False
    >>> t('h/k')
    False
    >>> t('i')
    True
    >>> t('i/a1')
    True
    >>> t('i/b2')
    False
    >>> t('i/a/b2/c3')
    'all'
    >>> t('i/a/b2/d4')
    False
    >>> t('i/a5/b7/d')
    'all'
    >>> t('j/x/y')
    True
    >>> t('z')
    False
    """
    tree = _tree()
    for kind, pat, _source in kindpats:
        if kind == "glob":
            components = []
            for p in pat.split("/"):
                if "[" in p or "{" in p or "*" in p or "?" in p:
                    break
                components.append(p)
            prefix = "/".join(components)
            matchrecursive = prefix == pat
            tree.insert(
                prefix,
                matchrecursive=matchrecursive,
                globpats=_remainingpats(pat, prefix),
            )
        elif kind == "re":
            # Still try to get a plain prefix from the regular expression so we
            # can still have fast paths.
            if pat.startswith("^"):
                # "re" already matches from the beginning, unlike "relre"
                pat = pat[1:]
            components = []
            for p in pat.split("/"):
                if re.escape(p) != p:
                    # contains special characters
                    break
                components.append(p)
            prefix = "/".join(components)
            tree.insert(
                prefix, matchrecursive=False, repats=_remainingpats(pat, prefix)
            )
        else:
            # Unsupported kind
            return None
    return tree.visitdir


class basematcher(object):
    def __init__(self, root, cwd, badfn=None, relativeuipath=True):
        self._root = root
        self._cwd = cwd
        if badfn is not None:
            self.bad = badfn
        self._relativeuipath = relativeuipath

    def __repr__(self):
        return "<%s>" % self.__class__.__name__

    def __call__(self, fn):
        return self.matchfn(fn)

    def __iter__(self):
        for f in self._files:
            yield f

    # Callbacks related to how the matcher is used by dirstate.walk.
    # Subscribers to these events must monkeypatch the matcher object.
    def bad(self, f, msg):
        """Callback from dirstate.walk for each explicit file that can't be
        found/accessed, with an error message."""

    # If an traversedir is set, it will be called when a directory discovered
    # by recursive traversal is visited.
    traversedir = None

    def abs(self, f):
        """Convert a repo path back to path that is relative to the root of the
        matcher."""
        return f

    def rel(self, f):
        """Convert repo path back to path that is relative to cwd of matcher."""
        return util.pathto(self._root, self._cwd, f)

    def uipath(self, f):
        """Convert repo path to a display path.  If patterns or -I/-X were used
        to create this matcher, the display path will be relative to cwd.
        Otherwise it is relative to the root of the repo."""
        return (self._relativeuipath and self.rel(f)) or self.abs(f)

    @propertycache
    def _files(self):
        return []

    def files(self):
        """Explicitly listed files or patterns or roots:
        if no patterns or .always(): empty list,
        if exact: list exact files,
        if not .anypats(): list all files and dirs,
        else: optimal roots"""
        return self._files

    @propertycache
    def _fileset(self):
        return set(self._files)

    def exact(self, f):
        """Returns True if f is in .files()."""
        return f in self._fileset

    def matchfn(self, f):
        return False

    def visitdir(self, dir):
        """Decides whether a directory should be visited based on whether it
        has potential matches in it or one of its subdirectories. This is
        based on the match's primary, included, and excluded patterns.

        Returns the string 'all' if the given directory and all subdirectories
        should be visited. Otherwise returns True or False indicating whether
        the given directory should be visited.
        """
        return True

    def always(self):
        """Matcher will match everything and .files() will be empty --
        optimization might be possible."""
        return False

    def isexact(self):
        """Matcher will match exactly the list of files in .files() --
        optimization might be possible."""
        return False

    def prefix(self):
        """Matcher will match the paths in .files() recursively --
        optimization might be possible."""
        return False

    def anypats(self):
        """None of .always(), .isexact(), and .prefix() is true --
        optimizations will be difficult."""
        return not self.always() and not self.isexact() and not self.prefix()


class alwaysmatcher(basematcher):
    """Matches everything."""

    def __init__(self, root, cwd, badfn=None, relativeuipath=False):
        super(alwaysmatcher, self).__init__(
            root, cwd, badfn, relativeuipath=relativeuipath
        )

    def always(self):
        return True

    def matchfn(self, f):
        return True

    def visitdir(self, dir):
        return "all"

    def __repr__(self):
        return "<alwaysmatcher>"


class nevermatcher(basematcher):
    """Matches nothing."""

    def __init__(self, root, cwd, badfn=None):
        super(nevermatcher, self).__init__(root, cwd, badfn)

    # It's a little weird to say that the nevermatcher is an exact matcher
    # or a prefix matcher, but it seems to make sense to let callers take
    # fast paths based on either. There will be no exact matches, nor any
    # prefixes (files() returns []), so fast paths iterating over them should
    # be efficient (and correct).
    def isexact(self):
        return True

    def prefix(self):
        return True

    def visitdir(self, dir):
        return False

    def __repr__(self):
        return "<nevermatcher>"


class gitignorematcher(basematcher):
    """Match files specified by ".gitignore"s"""

    def __init__(self, root, cwd, badfn=None, gitignorepaths=None):
        super(gitignorematcher, self).__init__(root, cwd, badfn)
        gitignorepaths = gitignorepaths or []
        self._matcher = pathmatcher.gitignorematcher(root, gitignorepaths)

    def matchfn(self, f):
        # XXX: is_dir is set to True here for performance.
        # It should be set to whether "f" is actually a directory or not.
        return self._matcher.match_relative(f, True)

    def explain(self, f):
        return self._matcher.explain(f, True)

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        matched = self._matcher.match_relative(dir, True)
        if matched:
            # Everything in the directory is selected (ignored)
            return "all"
        else:
            # Not sure
            return True

    def __repr__(self):
        return "<gitignorematcher>"


class treematcher(basematcher):
    """Match glob patterns with negative pattern support.
    Have a smarter 'visitdir' implementation.
    """

    def __init__(self, root, cwd, badfn=None, rules=[]):
        super(treematcher, self).__init__(root, cwd, badfn)
        rules = list(rules)
        self._matcher = pathmatcher.treematcher(rules)
        self._rules = rules

    def matchfn(self, f):
        return self._matcher.matches(f)

    def visitdir(self, dir):
        matched = self._matcher.match_recursive(dir)
        if matched is None:
            return True
        elif matched is True:
            return "all"
        else:
            assert matched is False
            return False

    def __repr__(self):
        return "<treematcher rules=%r>" % self._rules


def normalizerootdir(dir, funcname):
    if dir == ".":
        util.nouideprecwarn(
            "match.%s() no longer accepts '.', use '' instead." % funcname, "20190805"
        )
        return ""
    return dir


class patternmatcher(basematcher):
    def __init__(self, root, cwd, kindpats, ctx=None, badfn=None):
        super(patternmatcher, self).__init__(root, cwd, badfn)

        self._files = _explicitfiles(kindpats)
        self._prefix = _prefix(kindpats)
        self._pats, self.matchfn = _buildmatch(ctx, kindpats, "$", root)

    @propertycache
    def _dirs(self):
        return set(util.dirs(self._fileset))

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        if self._prefix and dir in self._fileset:
            return "all"
        if not self._prefix:
            return True
        return (
            dir in self._fileset
            or dir in self._dirs
            or any(parentdir in self._fileset for parentdir in util.finddirs(dir))
        )

    def prefix(self):
        return self._prefix

    def __repr__(self):
        return "<patternmatcher patterns=%r>" % self._pats


class includematcher(basematcher):
    def __init__(self, root, cwd, kindpats, ctx=None, badfn=None):
        super(includematcher, self).__init__(root, cwd, badfn)

        self._pats, self.matchfn = _buildmatch(ctx, kindpats, "(?:/|$)", root)
        # prefix is True if all patterns are recursive, so certain fast paths
        # can be enabled. Unfortunately, it's too easy to break it (ex. by
        # using "glob:*.c", "re:...", etc).
        self._prefix = _prefix(kindpats)
        roots, dirs = _rootsanddirs(kindpats)
        # roots are directories which are recursively included.
        # If self._prefix is True, then _roots can have a fast path for
        # visitdir to return "all", marking things included unconditionally.
        # If self._prefix is False, then that optimization is unsound because
        # "roots" might contain entries that is not recursive (ex. roots will
        # include "foo/bar" for pattern "glob:foo/bar/*.c").
        self._roots = set(roots)
        # dirs are directories which are non-recursively included.
        # That is, files under that directory are included. But not
        # subdirectories.
        self._dirs = set(dirs)
        # Try to use a more efficient visitdir implementation
        visitdir = _buildvisitdir(kindpats)
        if visitdir:
            self.visitdir = visitdir

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        if self._prefix and dir in self._roots:
            return "all"
        return (
            dir in self._roots
            or dir in self._dirs
            or any(parentdir in self._roots for parentdir in util.finddirs(dir))
        )

    def __repr__(self):
        return "<includematcher includes=%r>" % self._pats


class exactmatcher(basematcher):
    """Matches the input files exactly. They are interpreted as paths, not
    patterns (so no kind-prefixes).
    """

    def __init__(self, root, cwd, files, badfn=None):
        super(exactmatcher, self).__init__(root, cwd, badfn)

        if isinstance(files, list):
            self._files = files
        else:
            self._files = list(files)

    matchfn = basematcher.exact

    @propertycache
    def _dirs(self):
        return set(util.dirs(self._fileset))

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        return dir in self._dirs

    def isexact(self):
        return True

    def __repr__(self):
        return "<exactmatcher files=%r>" % self._files


class differencematcher(basematcher):
    """Composes two matchers by matching if the first matches and the second
    does not. Well, almost... If the user provides a pattern like "-X foo foo",
    Mercurial actually does match "foo" against that. That's because exact
    matches are treated specially. So, since this differencematcher is used for
    excludes, it needs to special-case exact matching.

    The second matcher's non-matching-attributes (root, cwd, bad, traversedir)
    are ignored.

    TODO: If we want to keep the behavior described above for exact matches, we
    should consider instead treating the above case something like this:
    union(exact(foo), difference(pattern(foo), include(foo)))
    """

    def __init__(self, m1, m2):
        super(differencematcher, self).__init__(m1._root, m1._cwd)
        self._m1 = m1
        self._m2 = m2
        self.bad = m1.bad
        self.traversedir = m1.traversedir

    def matchfn(self, f):
        return self._m1(f) and (not self._m2(f) or self._m1.exact(f))

    @propertycache
    def _files(self):
        if self.isexact():
            return [f for f in self._m1.files() if self(f)]
        # If m1 is not an exact matcher, we can't easily figure out the set of
        # files, because its files() are not always files. For example, if
        # m1 is "path:dir" and m2 is "rootfileins:.", we don't
        # want to remove "dir" from the set even though it would match m2,
        # because the "dir" in m1 may not be a file.
        return self._m1.files()

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        if self._m2.visitdir(dir) == "all":
            # There's a bug here: If m1 matches file 'dir/file' and m2 excludes
            # 'dir' (recursively), we should still visit 'dir' due to the
            # exception we have for exact matches.
            return False
        return bool(self._m1.visitdir(dir))

    def isexact(self):
        return self._m1.isexact()

    def __repr__(self):
        return "<differencematcher m1=%r, m2=%r>" % (self._m1, self._m2)


def intersectmatchers(m1, m2):
    """Composes two matchers by matching if both of them match.

    The second matcher's non-matching-attributes (root, cwd, bad, traversedir)
    are ignored.
    """
    if m1 is None or m2 is None:
        return m1 or m2
    if m1.always():
        m = copy.copy(m2)
        # TODO: Consider encapsulating these things in a class so there's only
        # one thing to copy from m1.
        m.bad = m1.bad
        m.traversedir = m1.traversedir
        m.abs = m1.abs
        m.rel = m1.rel
        m._relativeuipath |= m1._relativeuipath
        return m
    if m2.always():
        m = copy.copy(m1)
        m._relativeuipath |= m2._relativeuipath
        return m
    return intersectionmatcher(m1, m2)


class intersectionmatcher(basematcher):
    def __init__(self, m1, m2):
        super(intersectionmatcher, self).__init__(m1._root, m1._cwd)
        self._m1 = m1
        self._m2 = m2
        self.bad = m1.bad
        self.traversedir = m1.traversedir

    @propertycache
    def _files(self):
        if self.isexact():
            m1, m2 = self._m1, self._m2
            if not m1.isexact():
                m1, m2 = m2, m1
            return [f for f in m1.files() if m2(f)]
        # It neither m1 nor m2 is an exact matcher, we can't easily intersect
        # the set of files, because their files() are not always files. For
        # example, if intersecting a matcher "-I glob:foo.txt" with matcher of
        # "path:dir2", we don't want to remove "dir2" from the set.
        return self._m1.files() + self._m2.files()

    def matchfn(self, f):
        return self._m1(f) and self._m2(f)

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        visit1 = self._m1.visitdir(dir)
        if visit1 == "all":
            return self._m2.visitdir(dir)
        # bool() because visit1=True + visit2='all' should not be 'all'
        return bool(visit1 and self._m2.visitdir(dir))

    def always(self):
        return self._m1.always() and self._m2.always()

    def isexact(self):
        return self._m1.isexact() or self._m2.isexact()

    def __repr__(self):
        return "<intersectionmatcher m1=%r, m2=%r>" % (self._m1, self._m2)


class subdirmatcher(basematcher):
    """Adapt a matcher to work on a subdirectory only.

    The paths are remapped to remove/insert the path as needed:

    >>> from . import pycompat
    >>> m1 = match(b'root', b'', [b'a.txt', b'sub/b.txt'])
    >>> m2 = subdirmatcher(b'sub', m1)
    >>> bool(m2(b'a.txt'))
    False
    >>> bool(m2(b'b.txt'))
    True
    >>> bool(m2.matchfn(b'a.txt'))
    False
    >>> bool(m2.matchfn(b'b.txt'))
    True
    >>> m2.files()
    ['b.txt']
    >>> m2.exact(b'b.txt')
    True
    >>> util.pconvert(m2.rel(b'b.txt'))
    'sub/b.txt'
    >>> def bad(f, msg):
    ...     print(pycompat.sysstr(b"%s: %s" % (f, msg)))
    >>> m1.bad = bad
    >>> m2.bad(b'x.txt', b'No such file')
    sub/x.txt: No such file
    >>> m2.abs(b'c.txt')
    'sub/c.txt'
    """

    def __init__(self, path, matcher):
        super(subdirmatcher, self).__init__(matcher._root, matcher._cwd)
        self._path = path
        self._matcher = matcher
        self._always = matcher.always()

        self._files = [
            f[len(path) + 1 :] for f in matcher._files if f.startswith(path + "/")
        ]

        # If the parent repo had a path to this subrepo and the matcher is
        # a prefix matcher, this submatcher always matches.
        if matcher.prefix():
            self._always = any(f == path for f in matcher._files)

    def bad(self, f, msg):
        self._matcher.bad(self._path + "/" + f, msg)

    def abs(self, f):
        return self._matcher.abs(self._path + "/" + f)

    def rel(self, f):
        return self._matcher.rel(self._path + "/" + f)

    def uipath(self, f):
        return self._matcher.uipath(self._path + "/" + f)

    def matchfn(self, f):
        # Some information is lost in the superclass's constructor, so we
        # can not accurately create the matching function for the subdirectory
        # from the inputs. Instead, we override matchfn() and visitdir() to
        # call the original matcher with the subdirectory path prepended.
        return self._matcher.matchfn(self._path + "/" + f)

    def visitdir(self, dir):
        dir = normalizerootdir(dir, "visitdir")
        if dir == "":
            dir = self._path
        else:
            dir = self._path + "/" + dir
        return self._matcher.visitdir(dir)

    def always(self):
        return self._always

    def prefix(self):
        return self._matcher.prefix() and not self._always

    def __repr__(self):
        return "<subdirmatcher path=%r, matcher=%r>" % (self._path, self._matcher)


class unionmatcher(basematcher):
    """A matcher that is the union of several matchers.

    The non-matching-attributes (root, cwd, bad, traversedir) are
    taken from the first matcher.
    """

    def __init__(self, matchers):
        m1 = matchers[0]
        super(unionmatcher, self).__init__(m1._root, m1._cwd)
        self.traversedir = m1.traversedir
        self._matchers = matchers

    def matchfn(self, f):
        for match in self._matchers:
            if match(f):
                return True
        return False

    def visitdir(self, dir):
        r = False
        for m in self._matchers:
            v = m.visitdir(dir)
            if v == "all":
                return v
            r |= v
        return r

    def __repr__(self):
        return "<unionmatcher matchers=%r>" % self._matchers


class recursivematcher(basematcher):
    """Make matchers recursive. If "a/b/c" matches, match "a/b/c/**".

    It is intended to be used by hgignore only. Other matchers would want to
    fix "visitdir" and "matchfn" to take parent directories into consideration.
    """

    def __init__(self, matcher):
        self._matcher = matcher

    def matchfn(self, f):
        match = self._matcher
        return match(f) or any(map(match, util.dirs((f,))))

    def visitdir(self, dir):
        if self(dir):
            return "all"
        return self._matcher.visitdir(dir)

    def __repr__(self):
        return "<recursivematcher %r>" % self._matcher


def patkind(pattern, default=None):
    """If pattern is 'kind:pat' with a known kind, return kind."""
    return _patsplit(pattern, default)[0]


def _patsplit(pattern, default):
    """Split a string into the optional pattern kind prefix and the actual
    pattern."""
    if ":" in pattern:
        kind, pat = pattern.split(":", 1)
        if kind in allpatternkinds:
            return kind, pat
    return default, pattern


def _globre(pat):
    r"""Convert an extended glob string to a regexp string.

    >>> from . import pycompat
    >>> def bprint(s):
    ...     print(pycompat.sysstr(s))
    >>> bprint(_globre(br'?'))
    .
    >>> bprint(_globre(br'*'))
    [^/]*
    >>> bprint(_globre(br'**'))
    .*
    >>> bprint(_globre(br'**/a'))
    (?:.*/)?a
    >>> bprint(_globre(br'a/**/b'))
    a\/(?:.*/)?b
    >>> bprint(_globre(br'[a*?!^][^b][!c]'))
    [a*?!^][\^b][^c]
    >>> bprint(_globre(br'{a,b}'))
    (?:a|b)
    >>> bprint(_globre(br'.\*\?'))
    \.\*\?
    """
    i, n = 0, len(pat)
    res = ""
    group = 0
    escape = util.re.escape

    def peek():
        return i < n and pat[i : i + 1]

    while i < n:
        c = pat[i : i + 1]
        i += 1
        if c not in "*?[{},\\":
            res += escape(c)
        elif c == "*":
            if peek() == "*":
                i += 1
                if peek() == "/":
                    i += 1
                    res += "(?:.*/)?"
                else:
                    res += ".*"
            else:
                res += "[^/]*"
        elif c == "?":
            res += "."
        elif c == "[":
            j = i
            if j < n and pat[j : j + 1] in "!]":
                j += 1
            while j < n and pat[j : j + 1] != "]":
                j += 1
            if j >= n:
                res += "\\["
            else:
                stuff = pat[i:j].replace("\\", "\\\\")
                i = j + 1
                if stuff[0:1] == "!":
                    stuff = "^" + stuff[1:]
                elif stuff[0:1] == "^":
                    stuff = "\\" + stuff
                res = "%s[%s]" % (res, stuff)
        elif c == "{":
            group += 1
            res += "(?:"
        elif c == "}" and group:
            res += ")"
            group -= 1
        elif c == "," and group:
            res += "|"
        elif c == "\\":
            p = peek()
            if p:
                i += 1
                res += escape(p)
            else:
                res += escape(c)
        else:
            res += escape(c)
    return res


def _regex(kind, pat, globsuffix):
    """Convert a (normalized) pattern of any kind into a regular expression.
    globsuffix is appended to the regexp of globs."""
    if not pat and kind in ("glob", "relpath"):
        return ""
    if kind == "re":
        return pat
    if kind in ("path", "relpath"):
        if pat == ".":
            return ""
        return util.re.escape(pat) + "(?:/|$)"
    if kind == "rootfilesin":
        if pat == ".":
            escaped = ""
        else:
            # Pattern is a directory name.
            escaped = util.re.escape(pat) + "/"
        # Anything after the pattern must be a non-directory.
        return escaped + "[^/]+$"
    if kind == "relglob":
        return "(?:|.*/)" + _globre(pat) + globsuffix
    if kind == "relre":
        if pat.startswith("^"):
            return pat
        return ".*" + pat
    return _globre(pat) + globsuffix


def _buildmatch(ctx, kindpats, globsuffix, root):
    """Return regexp string and a matcher function for kindpats.
    globsuffix is appended to the regexp of globs."""
    matchfuncs = []

    subincludes, kindpats = _expandsubinclude(kindpats, root)
    if subincludes:
        submatchers = {}

        def matchsubinclude(f):
            for prefix, matcherargs in subincludes:
                if f.startswith(prefix):
                    mf = submatchers.get(prefix)
                    if mf is None:
                        mf = match(*matcherargs)
                        submatchers[prefix] = mf

                    if mf(f[len(prefix) :]):
                        return True
            return False

        matchfuncs.append(matchsubinclude)

    fset, kindpats = _expandsets(kindpats, ctx)
    if fset:
        matchfuncs.append(fset.__contains__)

    regex = ""
    if kindpats:
        regex, mf = _buildregexmatch(kindpats, globsuffix)
        matchfuncs.append(mf)

    if len(matchfuncs) == 1:
        return regex, matchfuncs[0]
    else:
        return regex, lambda f: any(mf(f) for mf in matchfuncs)


def _buildregexmatch(kindpats, globsuffix):
    """Build a match function from a list of kinds and kindpats,
    return regexp string and a matcher function."""
    try:
        regex = "(?:%s)" % "|".join(
            [_regex(k, p, globsuffix) for (k, p, s) in kindpats]
        )
        if len(regex) > 20000:
            raise OverflowError
        return regex, _rematcher(regex)
    except OverflowError:
        # We're using a Python with a tiny regex engine and we
        # made it explode, so we'll divide the pattern list in two
        # until it works
        l = len(kindpats)
        if l < 2:
            raise
        regexa, a = _buildregexmatch(kindpats[: l // 2], globsuffix)
        regexb, b = _buildregexmatch(kindpats[l // 2 :], globsuffix)
        return regex, lambda s: a(s) or b(s)
    except re.error:
        for k, p, s in kindpats:
            try:
                _rematcher("(?:%s)" % _regex(k, p, globsuffix))
            except re.error:
                if s:
                    raise error.Abort(_("%s: invalid pattern (%s): %s") % (s, k, p))
                else:
                    raise error.Abort(_("invalid pattern (%s): %s") % (k, p))
        raise error.Abort(_("invalid pattern"))


def _patternrootsanddirs(kindpats):
    """Returns roots and directories corresponding to each pattern.

    This calculates the roots and directories exactly matching the patterns and
    returns a tuple of (roots, dirs) for each. It does not return other
    directories which may also need to be considered, like the parent
    directories.
    """
    r = []
    d = []
    for kind, pat, source in kindpats:
        if kind == "glob":  # find the non-glob prefix
            root = []
            for p in pat.split("/"):
                if "[" in p or "{" in p or "*" in p or "?" in p:
                    break
                root.append(p)
            r.append("/".join(root))
        elif kind in ("relpath", "path"):
            if pat == ".":
                pat = ""
            r.append(pat)
        elif kind in ("rootfilesin",):
            if pat == ".":
                pat = ""
            d.append(pat)
        else:  # relglob, re, relre
            r.append("")
    return r, d


def _roots(kindpats):
    """Returns root directories to match recursively from the given patterns."""
    roots, dirs = _patternrootsanddirs(kindpats)
    return roots


def _rootsanddirs(kindpats):
    """Returns roots and exact directories from patterns.

    roots are directories to match recursively, whereas exact directories should
    be matched non-recursively. The returned (roots, dirs) tuple will also
    include directories that need to be implicitly considered as either, such as
    parent directories.

    >>> _rootsanddirs(
    ...     [(b'glob', b'g/h/*', b''), (b'glob', b'g/h', b''),
    ...      (b'glob', b'g*', b'')])
    (['g/h', 'g/h', ''], ['', 'g'])
    >>> _rootsanddirs(
    ...     [(b'rootfilesin', b'g/h', b''), (b'rootfilesin', b'', b'')])
    ([], ['g/h', '', '', 'g'])
    >>> _rootsanddirs(
    ...     [(b'relpath', b'r', b''), (b'path', b'p/p', b''),
    ...      (b'path', b'', b'')])
    (['r', 'p/p', ''], ['', 'p'])
    >>> _rootsanddirs(
    ...     [(b'relglob', b'rg*', b''), (b're', b're/', b''),
    ...      (b'relre', b'rr', b'')])
    (['', '', ''], [''])
    """
    r, d = _patternrootsanddirs(kindpats)

    # Append the parents as non-recursive/exact directories, since they must be
    # scanned to get to either the roots or the other exact directories.
    d.extend(sorted(util.dirs(d)))
    d.extend(sorted(util.dirs(r)))

    return r, d


def _explicitfiles(kindpats):
    """Returns the potential explicit filenames from the patterns.

    >>> _explicitfiles([(b'path', b'foo/bar', b'')])
    ['foo/bar']
    >>> _explicitfiles([(b'rootfilesin', b'foo/bar', b'')])
    []
    """
    # Keep only the pattern kinds where one can specify filenames (vs only
    # directory names).
    filable = [kp for kp in kindpats if kp[0] not in ("rootfilesin",)]
    return _roots(filable)


def _prefix(kindpats):
    """Whether all the patterns match a prefix (i.e. recursively)"""
    for kind, pat, source in kindpats:
        if kind not in ("path", "relpath"):
            return False
    return True


_commentre = None


def readpatternfile(filepath, warn, sourceinfo=False):
    """parse a pattern file, returning a list of
    patterns. These patterns should be given to compile()
    to be validated and converted into a match function.

    trailing white space is dropped.
    the escape character is backslash.
    comments start with #.
    empty lines are skipped.

    lines can be of the following formats:

    syntax: regexp # defaults following lines to non-rooted regexps
    syntax: glob   # defaults following lines to non-rooted globs
    re:pattern     # non-rooted regular expression
    glob:pattern   # non-rooted glob
    pattern        # pattern of the current default type

    if sourceinfo is set, returns a list of tuples:
    (pattern, lineno, originalline). This is useful to debug ignore patterns.
    """

    syntaxes = {
        "re": "relre:",
        "regexp": "relre:",
        "glob": "relglob:",
        "include": "include",
        "subinclude": "subinclude",
    }
    syntax = "relre:"
    patterns = []

    fp = open(filepath, "rb")
    for lineno, line in enumerate(util.iterfile(fp), start=1):
        if "#" in line:
            global _commentre
            if not _commentre:
                _commentre = util.re.compile(br"((?:^|[^\\])(?:\\\\)*)#.*")
            # remove comments prefixed by an even number of escapes
            m = _commentre.search(line)
            if m:
                line = line[: m.end(1)]
            # fixup properly escaped comments that survived the above
            line = line.replace("\\#", "#")
        line = line.rstrip()
        if not line:
            continue

        if line.startswith("syntax:"):
            s = line[7:].strip()
            try:
                syntax = syntaxes[s]
            except KeyError:
                if warn:
                    warn(_("%s: ignoring invalid syntax '%s'\n") % (filepath, s))
            continue

        linesyntax = syntax
        for s, rels in syntaxes.iteritems():
            if line.startswith(rels):
                linesyntax = rels
                line = line[len(rels) :]
                break
            elif line.startswith(s + ":"):
                linesyntax = rels
                line = line[len(s) + 1 :]
                break
        if sourceinfo:
            patterns.append((linesyntax + line, lineno, line))
        else:
            patterns.append(linesyntax + line)
    fp.close()
    return patterns
