# dagparser.py - parser and generator for concise description of DAGs
#
# Copyright 2010 Peter Arrenbrecht <peter@arrenbrecht.ch>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re
import string

from edenscmnative.bindings import vlq

from . import error, pycompat, util
from .i18n import _


try:
    xrange(0)
except NameError:
    xrange = range


def parsedag(desc):
    '''parses a DAG from a concise textual description; generates events

    "+n" is a linear run of n nodes based on the current default parent
    "." is a single node based on the current default parent
    "$" resets the default parent to -1 (implied at the start);
        otherwise the default parent is always the last node created
    "<p" sets the default parent to the backref p
    "*p" is a fork at parent p, where p is a backref
    "*p1/p2/.../pn" is a merge of parents p1..pn, where the pi are backrefs
    "/p2/.../pn" is a merge of the preceding node and p2..pn
    ":name" defines a label for the preceding node; labels can be redefined
    "@text" emits an annotation event for text
    "!command" emits an action event for the current node
    "!!my command\n" is like "!", but to the end of the line
    "#...\n" is a comment up to the end of the line

    Whitespace between the above elements is ignored.

    A backref is either
     * a number n, which references the node curr-n, where curr is the current
       node, or
     * the name of a label you placed earlier using ":name", or
     * empty to denote the default parent.

    All string valued-elements are either strictly alphanumeric, or must
    be enclosed in double quotes ("..."), with "\" as escape character.

    Generates sequence of

      ('n', (id, [parentids])) for node creation
      ('l', (id, labelname)) for labels on nodes
      ('a', text) for annotations
      ('c', command) for actions (!)
      ('C', command) for line actions (!!)

    Examples
    --------

    Example of a complex graph (output not shown for brevity):

        >>> len(list(parsedag(b"""
        ...
        ... +3         # 3 nodes in linear run
        ... :forkhere  # a label for the last of the 3 nodes from above
        ... +5         # 5 more nodes on one branch
        ... :mergethis # label again
        ... <forkhere  # set default parent to labeled fork node
        ... +10        # 10 more nodes on a parallel branch
        ... @stable    # following nodes will be annotated as "stable"
        ... +5         # 5 nodes in stable
        ... !addfile   # custom command; could trigger new file in next node
        ... +2         # two more nodes
        ... /mergethis # merge last node with labeled node
        ... +4         # 4 more nodes descending from merge node
        ...
        ... """)))
        34

    Empty list:

        >>> list(parsedag(b""))
        []

    A simple linear run:

        >>> list(parsedag(b"+3"))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [1]))]

    Some non-standard ways to define such runs:

        >>> list(parsedag(b"+1+2"))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [1]))]

        >>> list(parsedag(b"+1*1*"))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [1]))]

        >>> list(parsedag(b"*"))
        [('n', (0, [-1]))]

        >>> list(parsedag(b"..."))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [1]))]

    A fork and a join, using numeric back references:

        >>> list(parsedag(b"+2*2*/2"))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [0])), ('n', (3, [2, 1]))]

        >>> list(parsedag(b"+2<2+1/2"))
        [('n', (0, [-1])), ('n', (1, [0])), ('n', (2, [0])), ('n', (3, [2, 1]))]

    Placing a label:

        >>> list(parsedag(b"+1 :mylabel +1"))
        [('n', (0, [-1])), ('l', (0, 'mylabel')), ('n', (1, [0]))]

    An empty label (silly, really):

        >>> list(parsedag(b"+1:+1"))
        [('n', (0, [-1])), ('l', (0, '')), ('n', (1, [0]))]

    Fork and join, but with labels instead of numeric back references:

        >>> list(parsedag(b"+1:f +1:p2 *f */p2"))
        [('n', (0, [-1])), ('l', (0, 'f')), ('n', (1, [0])), ('l', (1, 'p2')),
         ('n', (2, [0])), ('n', (3, [2, 1]))]

        >>> list(parsedag(b"+1:f +1:p2 <f +1 /p2"))
        [('n', (0, [-1])), ('l', (0, 'f')), ('n', (1, [0])), ('l', (1, 'p2')),
         ('n', (2, [0])), ('n', (3, [2, 1]))]

    Restarting from the root:

        >>> list(parsedag(b"+1 $ +1"))
        [('n', (0, [-1])), ('n', (1, [-1]))]

    Annotations, which are meant to introduce sticky state for subsequent nodes:

        >>> list(parsedag(b"+1 @ann +1"))
        [('n', (0, [-1])), ('a', 'ann'), ('n', (1, [0]))]

        >>> list(parsedag(b'+1 @"my annotation" +1'))
        [('n', (0, [-1])), ('a', 'my annotation'), ('n', (1, [0]))]

    Commands, which are meant to operate on the most recently created node:

        >>> list(parsedag(b"+1 !cmd +1"))
        [('n', (0, [-1])), ('c', 'cmd'), ('n', (1, [0]))]

        >>> list(parsedag(b'+1 !"my command" +1'))
        [('n', (0, [-1])), ('c', 'my command'), ('n', (1, [0]))]

        >>> list(parsedag(b'+1 !!my command line\\n +1'))
        [('n', (0, [-1])), ('C', 'my command line'), ('n', (1, [0]))]

    Comments, which extend to the end of the line:

        >>> list(parsedag(b'+1 # comment\\n+1'))
        [('n', (0, [-1])), ('n', (1, [0]))]

    Error:

        >>> try: list(parsedag(b'+1 bad'))
        ... except Exception as e: print(pycompat.sysstr(bytes(e)))
        invalid character in dag description: bad...

    '''
    if not desc:
        return

    wordchars = pycompat.bytestr(string.ascii_letters + string.digits)

    labels = {}
    p1 = -1
    r = 0

    def resolve(ref):
        if not ref:
            return p1
        elif ref[0] in pycompat.bytestr(string.digits):
            return r - int(ref)
        else:
            return labels[ref]

    chiter = pycompat.iterbytestr(desc)

    def nextch():
        return next(chiter, "\0")

    def nextrun(c, allow):
        s = ""
        while c in allow:
            s += c
            c = nextch()
        return c, s

    def nextdelimited(c, limit, escape):
        s = ""
        while c != limit:
            if c == escape:
                c = nextch()
            s += c
            c = nextch()
        return nextch(), s

    def nextstring(c):
        if c == '"':
            return nextdelimited(nextch(), '"', "\\")
        else:
            return nextrun(c, wordchars)

    c = nextch()
    while c != "\0":
        while c in pycompat.bytestr(string.whitespace):
            c = nextch()
        if c == ".":
            yield "n", (r, [p1])
            p1 = r
            r += 1
            c = nextch()
        elif c == "+":
            c, digs = nextrun(nextch(), pycompat.bytestr(string.digits))
            n = int(digs)
            for i in xrange(0, n):
                yield "n", (r, [p1])
                p1 = r
                r += 1
        elif c in "*/":
            if c == "*":
                c = nextch()
            c, pref = nextstring(c)
            prefs = [pref]
            while c == "/":
                c, pref = nextstring(nextch())
                prefs.append(pref)
            ps = [resolve(ref) for ref in prefs]
            yield "n", (r, ps)
            p1 = r
            r += 1
        elif c == "<":
            c, ref = nextstring(nextch())
            p1 = resolve(ref)
        elif c == ":":
            c, name = nextstring(nextch())
            labels[name] = p1
            yield "l", (p1, name)
        elif c == "@":
            c, text = nextstring(nextch())
            yield "a", text
        elif c == "!":
            c = nextch()
            if c == "!":
                cmd = ""
                c = nextch()
                while c not in "\n\r\0":
                    cmd += c
                    c = nextch()
                yield "C", cmd
            else:
                c, cmd = nextstring(c)
                yield "c", cmd
        elif c == "#":
            while c not in "\n\r\0":
                c = nextch()
        elif c == "$":
            p1 = -1
            c = nextch()
        elif c == "\0":
            return  # in case it was preceded by whitespace
        else:
            s = ""
            i = 0
            while c != "\0" and i < 10:
                s += c
                i += 1
                c = nextch()
            raise error.Abort(_("invalid character in dag description: " "%s...") % s)


def dagtextlines(
    events,
    addspaces=True,
    wraplabels=False,
    wrapannotations=False,
    wrapcommands=False,
    wrapnonlinear=False,
    usedots=False,
    maxlinewidth=70,
):
    """generates single lines for dagtext()"""

    def wrapstring(text):
        if re.match("^[0-9a-z]*$", text):
            return text
        return '"' + text.replace("\\", "\\\\").replace('"', '"') + '"'

    def gen():
        labels = {}
        run = 0
        wantr = 0
        needroot = False
        for kind, data in events:
            if kind == "n":
                r, ps = data

                # sanity check
                if r != wantr:
                    raise error.Abort(_("expected id %i, got %i") % (wantr, r))
                if not ps:
                    ps = [-1]
                else:
                    for p in ps:
                        if p >= r:
                            raise error.Abort(
                                _("parent id %i is larger than " "current id %i")
                                % (p, r)
                            )
                wantr += 1

                # new root?
                p1 = r - 1
                if len(ps) == 1 and ps[0] == -1:
                    if needroot:
                        if run:
                            yield "+%d" % run
                            run = 0
                        if wrapnonlinear:
                            yield "\n"
                        yield "$"
                        p1 = -1
                    else:
                        needroot = True
                if len(ps) == 1 and ps[0] == p1:
                    if usedots:
                        yield "."
                    else:
                        run += 1
                else:
                    if run:
                        yield "+%d" % run
                        run = 0
                    if wrapnonlinear:
                        yield "\n"
                    prefs = []
                    for p in ps:
                        if p == p1:
                            prefs.append("")
                        elif p in labels:
                            prefs.append(labels[p])
                        else:
                            prefs.append("%d" % (r - p))
                    yield "*" + "/".join(prefs)
            else:
                if run:
                    yield "+%d" % run
                    run = 0
                if kind == "l":
                    rid, name = data
                    labels[rid] = name
                    yield ":" + name
                    if wraplabels:
                        yield "\n"
                elif kind == "c":
                    yield "!" + wrapstring(data)
                    if wrapcommands:
                        yield "\n"
                elif kind == "C":
                    yield "!!" + data
                    yield "\n"
                elif kind == "a":
                    if wrapannotations:
                        yield "\n"
                    yield "@" + wrapstring(data)
                elif kind == "#":
                    yield "#" + data
                    yield "\n"
                else:
                    raise error.Abort(
                        _("invalid event type in dag: " "('%s', '%s')")
                        % (util.escapestr(kind), util.escapestr(data))
                    )
        if run:
            yield "+%d" % run

    line = ""
    for part in gen():
        if part == "\n":
            if line:
                yield line
                line = ""
        else:
            if len(line) + len(part) >= maxlinewidth:
                yield line
                line = ""
            elif addspaces and line and part != ".":
                line += " "
            line += part
    if line:
        yield line


def dagtext(
    dag,
    addspaces=True,
    wraplabels=False,
    wrapannotations=False,
    wrapcommands=False,
    wrapnonlinear=False,
    usedots=False,
    maxlinewidth=70,
):
    """generates lines of a textual representation for a dag event stream

    events should generate what parsedag() does, so:

      ('n', (id, [parentids])) for node creation
      ('l', (id, labelname)) for labels on nodes
      ('a', text) for annotations
      ('c', text) for commands
      ('C', text) for line commands ('!!')
      ('#', text) for comment lines

    Parent nodes must come before child nodes.

    Examples
    --------

    Linear run:

        >>> dagtext([(b'n', (0, [-1])), (b'n', (1, [0]))])
        '+2'

    Two roots:

        >>> dagtext([(b'n', (0, [-1])), (b'n', (1, [-1]))])
        '+1 $ +1'

    Fork and join:

        >>> dagtext([(b'n', (0, [-1])), (b'n', (1, [0])), (b'n', (2, [0])),
        ...          (b'n', (3, [2, 1]))])
        '+2 *2 */2'

    Fork and join with labels:

        >>> dagtext([(b'n', (0, [-1])), (b'l', (0, b'f')), (b'n', (1, [0])),
        ...          (b'l', (1, b'p2')), (b'n', (2, [0])), (b'n', (3, [2, 1]))])
        '+1 :f +1 :p2 *f */p2'

    Annotations:

        >>> dagtext([(b'n', (0, [-1])), (b'a', b'ann'), (b'n', (1, [0]))])
        '+1 @ann +1'

        >>> dagtext([(b'n', (0, [-1])),
        ...          (b'a', b'my annotation'),
        ...          (b'n', (1, [0]))])
        '+1 @"my annotation" +1'

    Commands:

        >>> dagtext([(b'n', (0, [-1])), (b'c', b'cmd'), (b'n', (1, [0]))])
        '+1 !cmd +1'

        >>> dagtext([(b'n', (0, [-1])),
        ...          (b'c', b'my command'),
        ...          (b'n', (1, [0]))])
        '+1 !"my command" +1'

        >>> dagtext([(b'n', (0, [-1])),
        ...          (b'C', b'my command line'),
        ...          (b'n', (1, [0]))])
        '+1 !!my command line\\n+1'

    Comments:

        >>> dagtext([(b'n', (0, [-1])), (b'#', b' comment'), (b'n', (1, [0]))])
        '+1 # comment\\n+1'

        >>> dagtext([])
        ''

    Combining parsedag and dagtext:

        >>> dagtext(parsedag(b'+1 :f +1 :p2 *f */p2'))
        '+1 :f +1 :p2 *f */p2'

    """
    return "\n".join(
        dagtextlines(
            dag,
            addspaces,
            wraplabels,
            wrapannotations,
            wrapcommands,
            wrapnonlinear,
            usedots,
            maxlinewidth,
        )
    )


def bindag(revs, parentrevs):
    """Generate binary representation for a dag

    revs is a list of commit identities. It must be topo-sorted from the oldest
    to the newest commits.

    parentrevs is a function that takes a commit identity, and returns a list
    of parent commit identities: (rev) -> [rev].

    The binary format consists of a stream of VLQ-encoded integers.

    Every commit has an ID. The first commit created has ID K, the second has
    ID K+1, and so on. K does not matter, because the format uses relative
    reference to previous commits.

    To parse the binary data, read integers one by one, and handle them using
    the following rules:

    - 0: New root commit.
         Create a new commit that has no parents.
    - 1: New single-parent commit.
         Read the next integer as P. Create a new commit with a single parent
         with ID = <last ID> - P.
    - 2: New merge commit.
         Read the next two integers as P, Q. Create a new commit with two
         parents <last ID> - P, and <last ID> - Q.
    - 3: New merge commit (fast path 1).
         Read the next integer as Q. Create a new commit with two parents:
         <last ID>, and <last ID> - Q.
    - 4: New merge commit (fast path 2).
         Read the next integer as P. Create a new commit with two parents:
         <last ID> - P, and <last ID>.
    - N: New linear stack of commits (N > 4).
         Create a stack of N - 4 commits on top of the last commit created.
    """

    idmap = {}  # {rev: commit id}
    buf = util.stringio()

    def push(value, encode=vlq.encode, write=buf.write):
        """Append an integer to the buffer"""
        write(encode(value))

    pendingcommits = [0]

    def pushpending(push=push):
        if pendingcommits[0] > 0:
            push(pendingcommits[0] + 4)
            pendingcommits[0] = 0

    for rev in revs:
        nextid = len(idmap)
        idmap[rev] = nextid
        p1, p2 = parentrevs(rev)
        if p1 == -1:
            assert p2 == -1
            pushpending()
            push(0)
            pendingcommits[0] = 0
        elif idmap[p1] + 1 == nextid and p2 == -1:
            pendingcommits[0] += 1
        else:
            pushpending()
            lastid = nextid - 1
            dp1 = lastid - idmap[p1]
            if p2 == -1:
                push(1)
                push(dp1)
            else:
                dp2 = lastid - idmap[p2]
                if dp1 == 0:
                    push(3)
                    push(dp2)
                elif dp2 == 0:
                    push(4)
                    push(dp1)
                else:
                    push(2)
                    push(dp1)
                    push(dp2)

    pushpending()

    return buf.getvalue()


def parsebindag(data):
    """Reverse of `bindag`. Translated binary DAG to revs and parentrevs.

    The returned revs use integer commit identities starting from 0.
    """

    def readiter(data, decodeat=vlq.decodeat):
        offset = 0
        while offset < len(data):
            value, size = decodeat(data, offset)
            yield value
            offset += size

    it = readiter(data)
    parents = []  # index: id, value: parentids
    append = parents.append

    # build dag in-memory
    while True:
        i = next(it, None)
        lastid = len(parents) - 1
        if i is None:
            break
        elif i == 0:
            append(())
        elif i == 1:
            p1 = lastid - next(it)
            append((p1,))
        elif i == 2:
            p1 = lastid - next(it)
            p2 = lastid - next(it)
            append((p1, p2))
        elif i == 3:
            p1 = lastid
            p2 = lastid - next(it)
            append((p1, p2))
        elif i == 4:
            p1 = lastid - next(it)
            p2 = lastid
            append((p1, p2))
        else:
            n = i - 4
            while n > 0:
                p1 = len(parents) - 1
                parents.append((p1,))
                n -= 1

    revs = range(len(parents))
    parentrevs = parents.__getitem__
    return revs, parentrevs
