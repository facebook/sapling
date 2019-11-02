# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import hashlib
import sys
from functools import partial


config = {"animated": False}

eprint = sys.stderr.write


def render(obj, w=sys.stdout.write):
    if isinstance(obj, str):
        w(obj + "\n")
    elif obj is None:
        pass
    elif callable(obj):
        render(obj(), w=w)
    else:
        for subobj in iter(obj):
            render(subobj, w=w)


def repeat(template, items):
    for item in items:
        yield template % item


def block(name, inner, params=""):
    yield r"\begin{%s}%s" % (name, params)
    yield inner
    yield r"\end{%s}" % name


def frame(inner, title=None, subtitle=None):
    def innerwrapper():
        if title is not None:
            yield r"\frametitle{%s}" % title
        if subtitle is not None:
            yield r"\framesubtitle{%s}" % subtitle
        yield inner

    yield block("frame", innerwrapper)


def itemlist(*items):
    def itemwrapper():
        for item in items:
            yield r"\item"
            yield item

    yield block("itemize", itemwrapper)


def emoji(codepoint):
    return r"{\fontspec{Noto Emoji}\selectfont\scriptsize\symbol{%s}}" % codepoint


def fruit(idx):
    return emoji(127815 + idx)


def tt(text):
    return r"\texttt{%s}" % text


def small(text):
    return r"\small{%s}" % text


def smaller(text):
    return r"\scriptsize{%s}" % text


def tiny(text):
    return r"\tiny{%s}" % text


def box(inner, options="center"):
    yield block("beamercolorbox", inner, params=r"[%s]{}" % options)


def columns(*cols, **kwds):
    widths = kwds.get("widths", [0.48, 0.48])
    yield block(
        "columns",
        (
            block("column", c, params=r"{%s\textwidth}" % widths[i])
            for i, c in enumerate(cols)
        ),
        params="[T]",
    )


def table(rows):
    params = r"{l%s}" % ("r" * (len(rows[0]) - 1))
    content = (
        r"\toprule"
        + "\n"
        + r" & ".join(rows[0])
        + r" \\ \midrule"
        + "\n"
        + "".join(r" & ".join(map(str, vs)) + r" \\" + "\n" for vs in rows[1:])
        + r"\bottomrule"
    )
    yield block("tabular", content, params=params)


def animated(*items, **kwargs):
    start = kwargs.get("start", 1)
    if config.get("animated"):
        for i, item in enumerate(items):
            yield r"\onslide<%d->{" % (start + i)
            yield item
            yield r"}"
    else:
        yield items


def dag(
    revs,
    parents=None,
    texts=None,
    highlighted=None,
    seglevel=0,
    startx=0,
    starty=0,
    drawstyle="",
    ypos=None,
    red=None,
    green=None,
    segoverride=None,
):
    revs = list(revs)
    parents = parents or {}
    texts = texts or {}
    red = red or set()
    green = green or set()
    ypos = ypos or {}
    yposforce = ypos
    segoverride = segoverride or {}
    if not isinstance(texts, dict):
        texts = dict(enumerate(texts))
    for i, rev in enumerate(revs):
        if i == 0:
            parents.setdefault(rev, [])
        else:
            parents.setdefault(rev, [rev - 1])
        texts.setdefault(rev, str(rev))

    heads = set()
    ypos = []
    yposrev = {}
    for i, rev in enumerate(revs):
        heads.add(rev)
        heads.difference_update(parents[rev])
        nodestyle = "node"
        if highlighted is not None:
            if rev in highlighted:
                nodestyle = "nodehl"
        if nodestyle == "node":
            if rev in green:
                nodestyle = "nodegreen"
        if nodestyle == "node":
            if rev in red:
                nodestyle = "nodered"
        x = i * 1 + startx
        if rev in yposforce:
            y = yposforce[rev]
        else:
            y = len(heads)
            # Special scan commits in between, and adjust y accordingly
            if parents[rev]:
                pmin = min(revs.index(p) for p in parents[rev])
                ys = ypos[(pmin + 1) :]
                if ys:
                    ymax = max(ys)
                    ymin = min(ys)
                    if ymax >= y and ymin <= y:
                        y = ymax + 1
        ypos.append(y)
        yposrev[rev] = y
        y = y - 0.4 + starty
        text = texts[rev]
        if len(text) >= 3:
            text = tiny(text)
        elif len(text) >= 2:
            text = smaller(text)
        yield r"\node [%s] at (%s, %s) (n%s) {%s};" % (nodestyle, x, y, rev, text)

    for rev in revs:
        for p in parents[rev]:
            drawstyles = filter(None, ["->", drawstyle])
            if yposrev[p] == yposrev[rev]:
                line = "--"
            elif yposrev[p] > yposrev[rev]:
                line = "-|"
                drawstyles.append("rounded corners=3mm")
            else:
                line = "|-"
                drawstyles.append("rounded corners=3mm")
            yield r"\draw[%s] (n%s) %s (n%s);" % (",".join(drawstyles), p, line, rev)

    # Segments
    y = 0 - starty
    for i in range(seglevel):
        if i == 0:
            segs = segflat(revs, parents)
        else:
            segs = seghighlevel(segs)
        yield drawsegs(
            segs,
            y="-%scm" % (y + i * 0.9),
            highlighted=highlighted,
            segoverride=segoverride,
        )
        if len(segs) <= 1:
            break


tikzpicture = partial(block, "tikzpicture", params=r"[line width=0.25mm, >=stealth]")
adjustbox = partial(
    block,
    "adjustbox",
    params=r"{max totalsize={.9\textwidth}{.68\textheight},center,margin=0cm 0.4cm}",
)
tikzstyle = r"""
\tikzstyle{node} = [circle, text centered, minimum size=0.7cm, draw=black, fill=white];
\tikzstyle{nodehl} = [circle, text centered, minimum size=0.7cm, draw=black, text=white, fill=blue2];
\tikzstyle{nodered} = [circle, text centered, minimum size=0.7cm, draw=black, text=white, fill=tomato1];
\tikzstyle{nodegreen} = [circle, text centered, minimum size=0.7cm, draw=black, text=white, fill=lime1];
""".strip()
tikz = lambda inner: adjustbox(tikzpicture([tikzstyle, inner]))


def tikzdag(*args, **kwds):
    return tikz(dag(*args, **kwds))


def segflat(revs, parentrevs):
    """([rev], {rev: parents}) -> [(start, end, parents)]"""
    start = None  # current segment
    segs = []  # [(start, end)]
    for rev in revs:
        prevs = parentrevs[rev]
        if prevs != [rev - 1]:
            # Start a new segment
            if start is not None:
                segs.append((start, rev - 1, parentrevs[start]))
            start = rev
    segs.append((start, revs[-1], sorted(parentrevs[start])))
    return segs


def seghighlevel(segs, segsize=3):
    """[(start, end, parents)] -> [(start, end, parents)]"""

    def hlparents(subsegs):
        hlstart = subsegs[0][0]
        hlps = []
        for start, end, ps in subsegs:
            for p in ps:
                if p < hlstart:
                    hlps.append(p)
        return sorted(hlps)

    hlsegs = []
    i = 0
    while i < len(segs):
        best = i
        heads = set()
        for j in range(i, min(i + segsize, len(segs))):
            heads.difference_update(segs[j][2])
            heads.add(segs[j][1])
            if len(heads) == 1:
                best = j
        hlsegs.append((segs[i][0], segs[best][1], hlparents(segs[i : best + 1])))
        i = best + 1

    return hlsegs


def drawsegs(segs, y="-1.3cm", highlighted=None, segoverride=None):
    segoverride = segoverride or {}
    nodestyle = "midway, below"
    linestyle = "color=blue2, line width=0.7mm"
    segname = sha1(str(segs))
    rulername = "v%s" % segname
    yield r"\coordinate  (%s) at (0, %s);" % (rulername, y)
    for start, end, prevs in segs:
        if (start, end) in segoverride:
            (start, end), line1a = segoverride[(start, end)]
        else:
            line1a = ""
        leftname = "vl%s%s" % (segname, start)
        rightname = "vr%s%s" % (segname, end)
        if start == end:
            line1 = "%s%s" % (tt(start), line1a)
            line2 = ""
        else:
            line1 = tt("%s:%s%s" % (start, end, line1a))
            if start + 1 == end and len(prevs) >= 2:
                line2 = "ps="
            else:
                line2 = "parents="
        text = (
            r"{\setstretch{0.6} \begin{tabular}{c} %s \\ \tiny{%s%s} \end{tabular}}"
            % (line1, line2, tt(str(sorted(prevs))))
        )
        yield r"\coordinate  (%s) at ($ (n%s) - (0.3, 0) $);" % (leftname, start)
        yield r"\coordinate  (%s) at ($ (n%s) + (0.3, 0) $);" % (rightname, end)
        if highlighted is None:
            linestyle = "color=blue2, line width=0.7mm"
        else:
            if ("%s:%s" % (start, end)) in highlighted:
                linestyle = "color=blue2, line width=0.7mm"
            else:
                linestyle = "color=gray1, line width=0.3mm"
        yield r"\draw[%s] (%s |- %s) -- (%s |- %s) node[%s] {%s};" % (
            linestyle,
            leftname,
            rulername,
            rightname,
            rulername,
            nodestyle,
            text,
        )


def sha1(s):
    if isinstance(s, str):
        s = s.encode("utf-8")
    h = hashlib.sha1()
    h.update(s)
    return h.hexdigest()[:5]


def slidegoal():
    yield animated(
        r"Improve $ O(changelog\footnotemark[2]) $ DAG operations.",
        itemlist(
            r"$ O(1) $-ish clone with edenfs",
            r"$ O(\log changelog) $-ish ancestor related DAG calculations",
        ),
        r"Improve $ O(changelog) $ set operations.",
        itemlist(tt("a + b"), tt("a - b"), tt(r"a \& b")),
    )
    yield r"\footnotetext[2]{Number of commits in the repo}",


def slidenongoal():
    yield animated(
        r"Filter operations. Expect they run on a small subset.",
        itemlist(
            tt("author(alice)"), tt("date(2017)"), tt("branchpoint()"), tt("merge()")
        ),
        r"File DAG operations. Expect server-side support (ex. fastlog).",
        itemlist(tt("follow(path, commit)")),
    )


def slideidentities():
    yield columns(
        [
            tikzdag(range(5), texts=[fruit(i) for i in range(5)]),
            box(tt("[%s]" % ", ".join(fruit(i) for i in range(5)))),
            box("$O(N)$ space"),
        ],
        [tikzdag([1, 2, 3, 4, 5]), box(tt("1:5")), box("$O(1)$ space")],
    )


def slidesegmentdef():
    yield r"A Segment contains a list of numbers. \\ Numbers are sorted topologically in DAG."
    yield tikz(
        r"\node (p0) [node] at (0, 0.8) {p0};"
        r"\node (p1) [node] at (-0.9, 0){p1};"
        r"\node (p2) [node] at (0, -0.8) {p2};"
        r"\node (s1) [rectangle, draw, minimum size=1.4cm] at (2.6, 0) {Segment Commits};"
        r"\draw[->] (p0) -- (s1);"
        r"\draw[->] (p1) -- (s1);"
        r"\draw[->] (p2) -- (s1);"
    )
    yield r"Parents of a segment: %s. Stored with the segment." % tt(
        "parents(segment) - segment"
    )


def slideflatsegmentdef():
    yield "Constraints:"
    yield itemlist(
        r"%s (using revision numbers)" % tt(r"x:y"),
        r"%s is %s" % (tt("heads(x:y)"), tt("y")),
        r"%s is %s" % (tt("roots(x:y)"), tt("x")),
        r"%s is empty" % (tt(r"(((x:y) - x) \& merge())")),
    )
    texts = ["x", r"x+1", r"x+2", "...", r"y-2", r"y-1", "y"]
    yield tikzdag(range(7), texts=texts)
    yield "Properties:"
    yield itemlist(
        r"Reduce DAG complexity to $ O(merges) $",
        r"Parents of %s are segment parents" % tt("x"),
        r"Parent information is loseless",
    )


dag1 = partial(dag, range(1, 13), {3: [], 5: [2, 4], 9: [7], 11: [8, 10]})


def slideflatsegmentexample():
    yield tikz(dag1(seglevel=1))


def slidehighlevelsegmentdef():
    yield "Constraints:"
    yield itemlist(
        r"%s (using revision numbers)" % tt(r"x:y"),
        r"%s is %s" % (tt("heads(x:y)"), tt("y")),
    )
    texts = ["x", "", "", "m", "", "r", "y"]
    yield tikzdag(range(7), parents={2: [0], 3: [1, 2], 5: [], 6: [4, 5]}, texts=texts)
    yield "Properties:"
    yield itemlist(
        r"Compress commits across merges",
        r"Parent information is lossy\footnotemark[2]",
    )
    yield r"\footnotetext[2]{Cannot get parents of arbitrary commit by high-level segments only.}"


def sectionhighlevelexamples():
    f = partial(frame, title=r"High-Level Segments", subtitle="Example")
    yield f(
        [
            tikz(dag1(seglevel=2)),
            r"Segment Size = 3. A high-level segment contains 3 lower-level segments at most.",
        ]
    )
    yield f(tikz(dag1(seglevel=3)))
    parents = {
        5: [2],
        7: [4, 6],
        12: [8],
        16: [13, 15],
        14: [6, 10],
        17: [11, 16],
        19: [],
        20: [5],
        21: [19, 20],
        22: [18, 21],
    }
    yield f(tikzdag(range(1, 24), parents, seglevel=4))


def sectionhighlevelancestorexamples():
    for content in [
        ["Selecting %s" % tt("::12"), tikz(dag1(highlighted={12, "1:12"}, seglevel=3))],
        [
            "Selecting %s" % tt("::11"),
            tikz(
                dag1(
                    highlighted={11, 10, 8, 7, "9:10", "1:8", "11:11"},
                    seglevel=2,
                    segoverride={(11, 12): ((11, 11), r"\footnotemark[2]")},
                )
            ),
            r"\footnotetext[2]{%s is generated from %s.}" % (tt("11:11"), tt("11:12")),
        ],
        [
            "Selecting %s" % tt("::10"),
            tikz(
                dag1(
                    highlighted={10, 7, 4, 2, "5:7", "9:10", "3:4", "1:2"},
                    seglevel=2,
                    segoverride={(5, 8): ((5, 7), r"\footnotemark[2]")},
                )
            ),
            r"\footnotetext[2]{%s is generated from %s.}" % (tt("5:7"), tt("5:8")),
        ],
        ["Selecting %s" % tt("::8"), tikz(dag1(highlighted={8, "1:8"}, seglevel=2))],
    ]:
        yield frame(
            content,
            title=r"High-Level Segments",
            subtitle="Example: Ancestors Selection",
        )
    yield frame(
        slidespans, title=r"High-Level Segments", subtitle="Example: Common Ancestor"
    )


def slidespans():
    yield columns(
        [
            r"\vspace{1.5cm}",
            r"%s \\" % tt("ancestor(10, 8)"),
            r"= %s \\" % (tt(r"max(::10\footnotemark[2]  \& ::8)")),
            r"= %s \\" % (tt(r"max((1:7+9:10) \& 1:8)")),
            r"= %s \\" % (tt("max(1:7)")),
            r"= %s \\" % (tt("7")),
        ],
        [
            tikz(
                [
                    r"\tikzstyle{node} = [circle, text centered, minimum size=0.7cm, draw=gray1, text=gray1, fill=white];",
                    dag1(
                        highlighted={"5:7", "9:10", "3:4", "1:2"},
                        seglevel=1,
                        segoverride={(5, 8): ((5, 7), "")},
                        drawstyle="gray1",
                    ),
                    dag1(
                        highlighted={"1:8"}, seglevel=2, starty=-3.5, drawstyle="gray1"
                    ),
                ]
            )
        ],
        widths=(0.35, 0.63),
    )
    yield r"\footnotetext[2]{%s can be lazy\footnotemark[3] to avoid %s and %s lookups.}" % (
        tt("::10"),
        tt("1:2"),
        tt("3:4"),
    )
    yield r"\footnotetext[3]{Laziness has a cost. Non-lazy version has $ O(\log changelog) $-ish overhead.}"


def slidenumberassign1():
    yield columns(
        tikzdag(
            range(1, 8), parents={7: [3, 6], 4: []}, seglevel=4, green={2, 3, 5, 6}
        ),
        tikzdag(
            range(1, 8),
            parents={3: [1], 5: [3], 7: [5, 6], 2: [], 4: [2], 6: [4]},
            ypos={3: 1, 5: 1},
            red={3, 4, 5, 6},
            seglevel=2,
        ),
    )
    yield r"Use Depth-First Search, not Breadth-First Search, from a merge."


def slidenumberassign2():
    def y(n):
        return 1.3 + n * 0.5

    yield columns(
        tikzdag(
            range(1, 8),
            parents={2: [], 3: [1, 2], 4: [], 5: [3, 4], 6: [], 7: [5, 6]},
            green={3, 5, 7},
            seglevel=3,
            texts=" ABCDEFG",
        ),
        tikzdag(
            range(1, 8),
            parents={4: [], 3: [], 2: [], 5: [1, 4], 6: [3, 5], 7: [6, 2]},
            ypos={2: y(3), 6: 1, 5: 1, 4: y(1), 3: y(2), 7: 1, 1: 1},
            red={5, 6, 7},
            seglevel=3,
            texts=" AFDBCEG",
        ),
    )
    yield r"Use Depth-First Search."


def slidenumberassign3():
    def y(n):
        return 1 + n * 0.7

    yield columns(
        tikzdag(
            range(1, 8),
            parents={1: [], 2: [], 3: [1, 2], 4: [2], 5: [3, 4], 6: [4], 7: [5, 6]},
            green={3, 5, 7},
            red={4, 6},
            seglevel=3,
            ypos={2: y(3), 4: y(2), 6: y(1)},
            texts=" ABCDEFG",
        ),
        tikzdag(
            range(1, 8),
            parents={1: [], 2: [1], 3: [2], 4: [], 5: [1, 4], 6: [2, 5], 7: [3, 6]},
            green={2, 3, 7},
            ypos={4: y(3), 5: y(2), 6: y(1)},
            texts=" BDFACEG",
            seglevel=3,
        ),
    )
    yield r"Pick parent branch with less merges first."


def slidenumberassignalgo():
    yield r"To assign a number for %s, check its parents." % tt("x"),
    yield itemlist(
        r"If all parents have numbers, assign the next available number to %s."
        % tt("x"),
        r"Otherwise, pick the parent branch with less merges. Assign it recursively.",
    )


def sectionnumberassignments():
    f = partial(frame, title=r"Assigning Numbers")
    yield f(slidenumberassign1, subtitle="Flat Segments")
    yield f(slidenumberassign2, subtitle="Merges")
    yield f(slidenumberassign3, subtitle="Merges")
    yield f(slidenumberassignalgo, subtitle="Algorithm")


def sliderealworldrepos():
    rows = [
        ["Repo", "Commits", "Flat", r"Level 2\footnotemark[2]", "Level 3", "Level 4"],
        ["fbsource", "millions", 19961, 1325, 87, 6],
        ["mozilla", "469k", 34179, 2390, 149, 8],
        ["cpython", "98k", 21744, 1367, 81, 4],
        ["pypy", "74k", 9008, 609, 39, 2],
        ["git", "55k", 23884, 1534, 90, 5],
        ["hg", "42k", 4654, 297, 17, 0],
    ]
    yield box(table(rows))
    yield r"\footnotetext[2]{Segment Size = 16. Last segment per level is removed.}",


def slidestorage():
    yield itemlist(
        [r"Number - Commit Hash Mapping", itemlist(r"Large", r"Stored Server-side")],
        [
            r"Commit Hash - Commit Data (user, date, message) Mapping",
            itemlist(r"Larger", r"Stored Server-side"),
        ],
        [
            r"Segments",
            itemlist(r"Tiny (<1MB)", r"Calculated Server-side", r"Stored Client-side"),
        ],
    )


def header():
    yield r"\documentclass[aspectratio=169]{beamer}"
    packages = ["adjustbox", "fontspec", "tikz", "setspace", "booktabs"]
    # https://tex.stackexchange.com/a/235024
    yield r"""\makeatletter
% save the meaning of \@footnotetext
\let\BEAMER@footnotetext\@footnotetext
\makeatother"""
    yield repeat(r"\usepackage{%s}", packages)
    yield r"""\makeatletter
% restore the meaning of \@footnotetext
\let\@footnotetext\BEAMER@footnotetext
% patch the relevant command to do single spacing in footnotes
\expandafter\patchcmd\csname beamerx@\string\beamer@framefootnotetext\endcsname
  {\reset@font}
  {\def\baselinestretch{\setspace@singlespace}\reset@font}
  {}{}
\makeatother"""
    # https://tex.stackexchange.com/a/830
    yield r"\renewcommand*{\thefootnote}{\fnsymbol{footnote}}"
    tikzlibs = ["arrows", "shapes", "calc"]
    yield repeat(r"\usetikzlibrary{%s}", tikzlibs)
    # beamer uses sans for math, override it to serif
    yield r"\usefonttheme[onlymath]{serif}"
    # beamer uses sans by default
    mainfont = "Roboto"
    yield r"\setsansfont{%s}" % mainfont
    colors = {
        "blue2": "5890FF",
        "gray1": "E9EAED",
        "lime1": "A3CE71",
        "tomato1": "FB724B",
    }
    yield repeat(r"\definecolor{%s}{HTML}{%s}", colors.items())
    yield r"\beamertemplatenavigationsymbolsempty"
    yield r"\setbeamercolor{titlelike}{fg=blue2}"
    yield r"\setbeamercolor{itemize item}{fg=blue2}"
    yield r"\setbeamercolor{itemize subitem}{fg=blue2}"

    metadata = {
        "title": "Segmented Changelog",
        "author": "Jun Wu",
        "institute": "Facebook",
        "date": "April 15, 2019",
    }
    yield repeat(r"\%s{%s}", metadata.items())


def slides():
    yield frame(r"\titlepage")
    yield frame(slidegoal, title=r"Goals")
    yield frame(slidenongoal, title=r"Non-goals")
    yield frame(slideidentities, title=r"Commit Identities")
    yield frame(slidesegmentdef, title=r"Segments", subtitle="Segments and parents")
    yield frame(slideflatsegmentdef, title=r"Flat Segments")
    yield frame(slideflatsegmentexample, title=r"Flat Segments", subtitle="Example")
    yield frame(slidehighlevelsegmentdef, title=r"High-Level Segments")
    yield sectionhighlevelexamples
    yield sectionhighlevelancestorexamples
    yield sectionnumberassignments
    yield frame(sliderealworldrepos, title=r"Real-world Repos")
    yield frame(slidestorage, title=r"New Structures")


def toplevel(func):
    yield header()
    yield block("document", func)


if __name__ == "__main__":
    render(toplevel(slides))
