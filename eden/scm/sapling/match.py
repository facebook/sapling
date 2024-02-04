# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# match.py - filename matching
#
#  Copyright 2008, 2009 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import copy
from typing import List, Optional

from bindings import pathmatcher

from . import error, identity, util
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
    "rootfilesin",
)

propertycache = util.propertycache


def match(
    root,
    cwd,
    patterns=None,
    include=None,
    exclude=None,
    default: str = "glob",
    ctx=None,
    warn=None,
    badfn=None,
    icasefs: bool = False,
):
    """build an object to match a set of file patterns

    arguments:
    root - the canonical root of the tree you're matching against
    cwd - the current working directory, if relevant
    patterns - patterns to find
    include - patterns to include (unless they are excluded)
    exclude - patterns to exclude (even if they are included)
    default - if a pattern in patterns has no explicit type, assume this one
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
    '<something>' - a pattern of the specified default type
    """

    hm = hintedmatcher(
        root,
        cwd,
        patterns or [],
        include or [],
        exclude or [],
        default,
        ctx,
        casesensitive=not icasefs,
        badfn=badfn,
    )
    if warn:
        for warning in hm.warnings():
            warn("warning: " + identity.replace(warning) + "\n")
    return hm


def exact(root, cwd, files, badfn=None) -> "exactmatcher":
    return exactmatcher(root, cwd, files, badfn=badfn)


def always(root, cwd) -> "alwaysmatcher":
    return alwaysmatcher(root, cwd)


def never(root, cwd) -> "nevermatcher":
    return nevermatcher(root, cwd)


def union(matches, root, cwd):
    """Union a list of matchers.

    If the list is empty, return nevermatcher.
    If the list only contains one non-None value, return that matcher.
    Otherwise return a union matcher.
    """
    matches = list(filter(None, matches))
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


class basematcher:
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
        """Matcher will match everything and .files() will be empty.
        Optimization might be possible."""
        return False

    def isexact(self):
        """Matcher matches exactly the list of files in .files(), and nothing else.
        Optimization might be possible."""
        return False

    def prefix(self):
        """Matcher matches the paths in .files() recursively, and nothing else.
        Optimization might be possible."""
        return False

    def anypats(self):
        """Matcher contains a non-trivial pattern (i.e. non-path and non-always).
        If this returns False, code assumes files() is all that matters.
        Optimizations will be difficult."""
        if self.always():
            # This is confusing since, conceptually, we are saying
            # there aren't patterns when we have a pattern like "**".
            # But since always() implies files() is empty, it is safe
            # for code to assume files() is all that's important.
            return False

        if self.isexact():
            # Only exacty files - no patterns.
            return False

        if self.prefix():
            # Only recursive paths - no patterns.
            return False

        return True


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
        self._matcher = pathmatcher.gitignorematcher(
            root, gitignorepaths, util.fscasesensitive(root)
        )

    def matchfn(self, f):
        return self._matcher.match_relative(f, False)

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

    def __init__(
        self,
        root,
        cwd,
        badfn=None,
        rules: Optional[List[str]] = None,
        ruledetails: Optional[List] = None,
        casesensitive=True,
        matcher: Optional[pathmatcher.treematcher] = None,
    ):
        super(treematcher, self).__init__(root, cwd, badfn)

        if (rules is None) == (matcher is None):
            raise error.ProgrammingError("must specify exactly one of rules or matcher")

        if rules is not None:
            rules = list(rules)
            self._matcher = pathmatcher.treematcher(rules, casesensitive)
            self._rules = rules
        else:
            assert matcher is not None
            self._matcher = matcher
            self._rules = None

        self._ruledetails = ruledetails or rules

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

    def explain(self, f):
        matchingidxs = self._matcher.matching_rule_indexes(f)
        if matchingidxs and self._ruledetails:
            # Use the final matching index (this follows the "last match wins"
            # logic within the tree matcher).
            return self._ruledetails[matchingidxs[-1]]
        return None

    def __repr__(self):
        return "<treematcher rules=%r>" % self._rules


class hintedmatcher(basematcher):
    """Rust matcher fully implementing Python API."""

    def __init__(
        self,
        root,
        cwd,
        patterns: List[str],
        include: List[str],
        exclude: List[str],
        default: str,
        ctx,
        casesensitive: bool,
        badfn=None,
    ):
        super(hintedmatcher, self).__init__(
            root, cwd, badfn, relativeuipath=bool(patterns or include or exclude)
        )

        def expandsets(pats, default):
            fset, nonsets = set(), []
            for pat in pats:
                k, p = _patsplit(pat, default)
                if k == "set":
                    if not ctx:
                        raise error.ProgrammingError(
                            "fileset expression with no " "context"
                        )
                    fset.update(ctx.getfileset(p))
                else:
                    nonsets.append(pat)

            if len(nonsets) == len(pats):
                return nonsets, None
            else:
                return nonsets, list(fset)

        self._matcher = pathmatcher.hintedmatcher(
            *expandsets(patterns, default),
            *expandsets(include, "glob"),
            *expandsets(exclude, "glob"),
            default,
            casesensitive,
            root,
            cwd,
        )
        self._files = self._matcher.exact_files()

    def matchfn(self, f):
        return self._matcher.matches_file(f)

    def visitdir(self, dir):
        matched = self._matcher.matches_directory(dir)
        if matched is None:
            return True
        elif matched is True:
            return "all"
        else:
            assert matched is False, f"expected False, but got {matched}"
            return False

    def always(self):
        return self._matcher.always_matches()

    def prefix(self):
        return self._matcher.all_recursive_paths()

    def isexact(self):
        # Similar to nevermatcher, let the knowledge that we never match
        # allow isexact() fast paths.
        return self._matcher.never_matches()

    def warnings(self):
        return self._matcher.warnings()


def normalizerootdir(dir: str, funcname) -> str:
    if dir == ".":
        util.nouideprecwarn(
            "match.%s() no longer accepts '.', use '' instead." % funcname, "20190805"
        )
        return ""
    return dir


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

    def explain(self, f):
        include_explains = []
        exclude_explains = []
        for match in self._matchers:
            explanation = match.explain(f)
            if explanation:
                if match(f):
                    include_explains.append(explanation)
                else:
                    exclude_explains.append(explanation)
        if include_explains:
            summary = "\n".join(include_explains)
            if exclude_explains:
                exclude_summary = "\n".join(
                    f"{e} (overridden by rules above)" for e in exclude_explains
                )
                summary += "\n" + exclude_summary
            return summary
        elif exclude_explains:
            exclude_summary = "\n".join(exclude_explains)
            return exclude_summary
        else:
            return None

    def __repr__(self):
        return "<unionmatcher matchers=%r>" % self._matchers


class xormatcher(basematcher):
    """A matcher that is the xor of two matchers i.e. match returns true if there's at least
    one false and one true.

    The non-matching-attributes (root, cwd, bad, traversedir) are
    taken from the first matcher.
    """

    def __init__(self, m1, m2):
        super(xormatcher, self).__init__(m1._root, m1._cwd)
        self.traversedir = m1.traversedir
        self.m1 = m1
        self.m2 = m2

    def matchfn(self, f):
        return bool(self.m1(f)) ^ bool(self.m2(f))

    def visitdir(self, dir):
        m1dir = self.m1.visitdir(dir)
        m2dir = self.m2.visitdir(dir)

        # if both matchers return "all" then we know for sure we don't need
        # to visit this directory. Same if all matchers return False. In all
        # other case we have to visit a directory.
        if m1dir == "all" and m2dir == "all":
            return False
        if not m1dir and not m2dir:
            return False
        return True

    def __repr__(self):
        return "<xormatcher m1=%r m2=%r>" % (self.m1, self.m2)


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
