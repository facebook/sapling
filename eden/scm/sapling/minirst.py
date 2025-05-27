# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# minirst.py - minimal reStructuredText parser
#
# Copyright 2009, 2010 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""simplified reStructuredText parser.

This parser knows just enough about reStructuredText to parse the
Mercurial docstrings.

It cheats in a major way: nested blocks are not really nested. They
are just indented blocks that look like they are nested. This relies
on the user to keep the right indentation for the blocks.

Remember to update https://mercurial-scm.org/wiki/HelpStyleGuide
when adding support for new constructs.
"""

import re
import sys

from . import encoding, identity, url, util
from .i18n import _


def section(s):
    return "%s\n%s\n\n" % (s, '"' * encoding.colwidth(s))


def subsection(s):
    return "%s\n%s\n\n" % (s, "=" * encoding.colwidth(s))


def subsubsection(s):
    return "%s\n%s\n\n" % (s, "-" * encoding.colwidth(s))


def subsubsubsection(s):
    return "%s\n%s\n\n" % (s, "." * encoding.colwidth(s))


def replace(text, substs):
    """
    Apply a list of (find, replace) pairs to a text.

    >>> replace("foo bar", [('f', 'F'), ('b', 'B')])
    'Foo Bar'
    >>> encoding.encoding = 'latin1'
    >>> replace('\\x81\\\\', [('\\\\', '/')])
    '\\x81/'
    """

    if sys.version_info[0] == 3:
        assert isinstance(text, str)
        for f, t in substs:
            text = text.replace(f, t)
        return text
    else:
        # some character encodings (cp932 for Japanese, at least) use
        # ASCII characters other than control/alphabet/digit as a part of
        # multi-bytes characters, so direct replacing with such characters
        # on strings in local encoding causes invalid byte sequences.
        utext = text.decode(encoding.encoding)
        for f, t in substs:
            utext = utext.replace(f.decode("ascii"), t.decode("ascii"))
        return utext.encode(encoding.encoding)


_blockre = re.compile(r"\n(?:\s*\n)+")


def findblocks(text):
    """Find continuous blocks of lines in text.

    Returns a list of dictionaries representing the blocks. Each block
    has an 'indent' field and a 'lines' field.
    """
    blocks = []
    for b in _blockre.split(text.lstrip("\n").rstrip()):
        lines = b.splitlines()
        if lines:
            indent = min((len(l) - len(l.lstrip())) for l in lines)
            lines = [l[indent:] for l in lines]
            blocks.append({"indent": indent, "lines": lines})
    return blocks


def findliteralblocks(blocks):
    """Finds literal blocks and adds a 'type' field to the blocks.

    Literal blocks are given the type 'literal', all other blocks are
    given type the 'paragraph'.
    """
    i = 0
    while i < len(blocks):
        # Searching for a block that looks like this:
        #
        # +------------------------------+
        # | paragraph                    |
        # | (ends with "::")             |
        # +------------------------------+
        #    +---------------------------+
        #    | indented literal block    |
        #    +---------------------------+
        blocks[i]["type"] = "paragraph"
        if blocks[i]["lines"][-1].endswith("::") and i + 1 < len(blocks):
            indent = blocks[i]["indent"]
            adjustment = blocks[i + 1]["indent"] - indent

            if blocks[i]["lines"] == ["::"]:
                # Expanded form: remove block
                del blocks[i]
                i -= 1
            elif blocks[i]["lines"][-1].endswith(" ::"):
                # Partially minimized form: remove space and both
                # colons.
                blocks[i]["lines"][-1] = blocks[i]["lines"][-1][:-3]
            elif (
                len(blocks[i]["lines"]) == 1
                and blocks[i]["lines"][0].lstrip(" ").startswith(".. ")
                and blocks[i]["lines"][0].find(" ", 3) == -1
            ):
                # directive on its own line, not a literal block
                i += 1
                continue
            else:
                # Fully minimized form: remove just one colon.
                blocks[i]["lines"][-1] = blocks[i]["lines"][-1][:-1]

            # List items are formatted with a hanging indent. We must
            # correct for this here while we still have the original
            # information on the indentation of the subsequent literal
            # blocks available.
            m = _bulletre.match(blocks[i]["lines"][0])
            if m:
                indent += m.end()
                adjustment -= m.end()

            # Mark the following indented blocks.
            while i + 1 < len(blocks) and blocks[i + 1]["indent"] > indent:
                blocks[i + 1]["type"] = "literal"
                blocks[i + 1]["indent"] -= adjustment
                i += 1
        i += 1
    return blocks


_bulletre = re.compile(r"(\*|-|[0-9A-Za-z]+\.|\(?[0-9A-Za-z]+\)|\|) ")
_optionre = re.compile(r"^(-([a-zA-Z0-9]), )?(--[a-z0-9-]+)((.*)  +)(.*)$")
_fieldre = re.compile(r":(?![: ])([^:]*)(?<! ):[ ]+(.*)")
_definitionre = re.compile(r"[^ ]")
_tablere = re.compile(r"(=+\s+)*=+")


def splitparagraphs(blocks):
    """Split paragraphs into lists."""
    # Tuples with (list type, item regexp, single line items?). Order
    # matters: definition lists has the least specific regexp and must
    # come last.
    listtypes = [
        ("bullet", _bulletre, True),
        ("option", _optionre, True),
        ("field", _fieldre, True),
        ("definition", _definitionre, False),
    ]

    def match(lines, i, itemre, singleline):
        """Does itemre match an item at line i?

        A list item can be followed by an indented line or another list
        item (but only if singleline is True).
        """
        line1 = lines[i]
        line2 = i + 1 < len(lines) and lines[i + 1] or ""
        if not itemre.match(line1):
            return False
        if singleline:
            return line2 == "" or line2[0] == " " or itemre.match(line2)
        else:
            return line2.startswith(" ")

    i = 0
    while i < len(blocks):
        if blocks[i]["type"] == "paragraph":
            lines = blocks[i]["lines"]
            for type, itemre, singleline in listtypes:
                if match(lines, 0, itemre, singleline):
                    items = []
                    for j, line in enumerate(lines):
                        if match(lines, j, itemre, singleline):
                            items.append(
                                {
                                    "type": type,
                                    "lines": [],
                                    "indent": blocks[i]["indent"],
                                }
                            )
                        items[-1]["lines"].append(line)
                    blocks[i : i + 1] = items
                    break
        i += 1
    return blocks


_fieldwidth = 14


def updatefieldlists(blocks):
    """Find key for field lists."""
    i = 0
    while i < len(blocks):
        if blocks[i]["type"] != "field":
            i += 1
            continue

        j = i
        while j < len(blocks) and blocks[j]["type"] == "field":
            m = _fieldre.match(blocks[j]["lines"][0])
            key, rest = m.groups()
            blocks[j]["lines"][0] = rest
            blocks[j]["key"] = key
            j += 1

        i = j + 1

    return blocks


def updateoptionlists(blocks):
    i = 0
    while i < len(blocks):
        if blocks[i]["type"] != "option":
            i += 1
            continue

        optstrwidth = 0
        j = i
        while j < len(blocks) and blocks[j]["type"] == "option":
            m = _optionre.match(blocks[j]["lines"][0])

            shortoption = m.group(2)
            group3 = m.group(3)
            longoption = group3[2:].strip()
            desc = m.group(6).strip()
            longoptionarg = m.group(5).strip()
            blocks[j]["lines"][0] = desc

            noshortop = ""
            if not shortoption:
                noshortop = "   "

            opt = "%s%s" % (
                shortoption and "-%s " % shortoption or "",
                "%s--%s %s" % (noshortop, longoption, longoptionarg),
            )
            opt = opt.rstrip()
            blocks[j]["optstr"] = opt
            optstrwidth = max(optstrwidth, encoding.colwidth(opt))
            j += 1

        for block in blocks[i:j]:
            block["optstrwidth"] = optstrwidth
        i = j + 1
    return blocks


def prunecontainers(blocks, keep):
    """Prune unwanted containers.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    pruned = []
    i = 0
    while i + 1 < len(blocks):
        # Searching for a block that looks like this:
        #
        # +-------+---------------------------+
        # | ".. container ::" type            |
        # +---+                               |
        #     | blocks                        |
        #     +-------------------------------+
        if blocks[i]["type"] == "paragraph" and blocks[i]["lines"][0].startswith(
            ".. container::"
        ):
            indent = blocks[i]["indent"]
            adjustment = blocks[i + 1]["indent"] - indent
            containertype = blocks[i]["lines"][0][15:]
            prune = True
            for c in keep:
                if c in containertype.split("."):
                    prune = False
            if prune:
                pruned.append(containertype)

            # Always delete "..container:: type" block
            del blocks[i]
            j = i
            i -= 1
            while j < len(blocks) and blocks[j]["indent"] > indent:
                if prune:
                    del blocks[j]
                else:
                    blocks[j]["indent"] -= adjustment
                    j += 1
        i += 1
    return blocks, pruned


_sectionre = re.compile(r"""^([-=`:.'"~^_*+#])\1+$""")


def findtables(blocks):
    """Find simple tables

    Only simple one-line table elements are supported
    """

    for block in blocks:
        # Searching for a block that looks like this:
        #
        # === ==== ===
        #  A    B   C
        # === ==== ===  <- optional
        #  1    2   3
        #  x    y   z
        # === ==== ===
        if (
            block["type"] == "paragraph"
            and len(block["lines"]) > 2
            and _tablere.match(block["lines"][0])
            and block["lines"][0] == block["lines"][-1]
        ):
            block["type"] = "table"
            block["header"] = False
            div = block["lines"][0]

            # column markers are ASCII so we can calculate column
            # position in bytes
            columns = [
                x
                for x in range(len(div))
                if div[x : x + 1] == "=" and (x == 0 or div[x - 1 : x] == " ")
            ]
            rows = []
            for l in block["lines"][1:-1]:
                if l == div:
                    block["header"] = True
                    continue
                row = []
                # we measure columns not in bytes or characters but in
                # colwidth which makes things tricky
                pos = columns[0]  # leading whitespace is bytes
                for n, start in enumerate(columns):
                    if n + 1 < len(columns):
                        width = columns[n + 1] - start
                        v = encoding.getcols(l, pos, width)  # gather columns
                        pos += len(v)  # calculate byte position of end
                        row.append(v.strip())
                    else:
                        row.append(l[pos:].strip())
                rows.append(row)

            block["table"] = rows

    return blocks


def findsections(blocks):
    """Finds sections.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    for block in blocks:
        # Searching for a block that looks like this:
        #
        # +------------------------------+
        # | Section title                |
        # | -------------                |
        # +------------------------------+
        if (
            block["type"] == "paragraph"
            and len(block["lines"]) == 2
            and encoding.colwidth(block["lines"][0]) <= len(block["lines"][1])
            and _sectionre.match(block["lines"][1])
        ):
            block["underline"] = block["lines"][1][0:1]
            block["type"] = "section"
            del block["lines"][1]
    return blocks


def inlineliterals(blocks, style="plain"):
    if style == "md":
        substs = [("``", "`")]
    else:
        substs = [("``", '"')]
    for b in blocks:
        if b["type"] in ("paragraph", "section"):
            b["lines"] = [replace(l, substs) for l in b["lines"]]
    return blocks


def hgrole(blocks, style="plain"):
    unisubst = list(identity.templatemap().items())
    cliname = identity.default().cliname()
    substs = [
        (":hg:`", "'%s " % cliname),
        (":prog:`", "'%s " % cliname),
        ("`", "'"),
    ] + unisubst
    for b in blocks:
        apply_all_substitutions = b["type"] in ("paragraph", "section")
        if apply_all_substitutions and style == "md":
            # In Markdown, it is not correct to assume backtick has been
            # rewritten as double-quote, so use a special multiline regex
            # instead of
            pattern = re.compile(r":(hg|prog):`([^\`]+)`", re.MULTILINE)
            body = "\n".join(b["lines"])
            startindex = 0
            while True:
                match = pattern.search(body, startindex)
                if not match:
                    break

                span = match.span()
                replacement = f"`{cliname} {match[2]}`"
                body = body[: span[0]] + replacement + body[span[1] :]
                startindex += len(replacement)
            b["lines"] = body.split("\n")
            b["lines"] = [replace(l, unisubst) for l in b["lines"]]
            continue

        stype = substs if apply_all_substitutions else unisubst
        # Turn :prog:`command` into "hg command". This also works
        # when there is a line break in the command and relies on
        # the fact that we have no stray back-quotes in the input
        # (run the blocks through inlineliterals first).
        b["lines"] = [replace(l, stype) for l in b["lines"]]
    return blocks


def addmargins(blocks):
    """Adds empty blocks for vertical spacing.

    This groups bullets, options, and definitions together with no vertical
    space between them, and adds an empty block between all other blocks.
    """
    i = 1
    while i < len(blocks):
        if blocks[i]["type"] == blocks[i - 1]["type"] and blocks[i]["type"] in (
            "bullet",
            "option",
            "field",
        ):
            i += 1
        elif not blocks[i - 1]["lines"]:
            # no lines in previous block, do not separate
            i += 1
        else:
            blocks.insert(i, {"lines": [""], "indent": 0, "type": "margin"})
            i += 2
    return blocks


def prunecomments(blocks):
    """Remove comments."""
    i = 0
    while i < len(blocks):
        b = blocks[i]
        if b["type"] == "paragraph" and (
            b["lines"][0].startswith(".. ") or b["lines"] == [".."]
        ):
            del blocks[i]
            if i < len(blocks) and blocks[i]["type"] == "margin":
                del blocks[i]
        else:
            i += 1
    return blocks


def findadmonitions(blocks, admonitions=None):
    """
    Makes the type of the block an admonition block if
    the first line is an admonition directive
    """
    admonitions = admonitions or _admonitiontitles.keys()

    admonitionre = re.compile(
        r"\.\. (%s)::" % "|".join(sorted(admonitions)), flags=re.IGNORECASE
    )

    i = 0
    while i < len(blocks):
        m = admonitionre.match(blocks[i]["lines"][0])
        if m:
            blocks[i]["type"] = "admonition"
            admonitiontitle = blocks[i]["lines"][0][3 : m.end() - 2].lower()

            firstline = blocks[i]["lines"][0][m.end() + 1 :]
            if firstline:
                blocks[i]["lines"].insert(1, "   " + firstline)

            blocks[i]["admonitiontitle"] = admonitiontitle
            del blocks[i]["lines"][0]
        i = i + 1
    return blocks


_admonitiontitles = {
    "attention": _("Attention:"),
    "caution": _("Caution:"),
    "danger": _("!Danger!"),
    "error": _("Error:"),
    "hint": _("Hint:"),
    "important": _("Important:"),
    "note": _("Note:"),
    "tip": _("Tip:"),
    "warning": _("Warning!"),
    "trigger-hint": "",
}


def formatoption(block, width):
    desc = " ".join(map(str.strip, block["lines"]))
    colwidth = encoding.colwidth(block["optstr"])
    usablewidth = width - 1
    hanging = block["optstrwidth"]
    initindent = "%s%s  " % (block["optstr"], " " * (hanging - colwidth))
    hangindent = " " * (encoding.colwidth(initindent) + 1)
    return " %s\n" % (
        util.wrap(desc, usablewidth, initindent=initindent, hangindent=hangindent)
    )


def formatblock(block, width):
    """Format a block according to width."""
    if width <= 0:
        width = 78
    indent = " " * block["indent"]
    if block["type"] == "admonition":
        admonitiontitle = block["admonitiontitle"]
        admonition = _admonitiontitles[admonitiontitle]
        if admonitiontitle == "trigger-hint":
            hintname = block["lines"][0].strip()
            from . import hintutil

            hintutil.trigger(hintname)
            return ""
        if not block["lines"]:
            return indent + admonition + "\n"
        hang = len(block["lines"][-1]) - len(block["lines"][-1].lstrip())

        defindent = indent + hang * " "
        text = " ".join(map(str.strip, block["lines"]))
        return "%s\n%s\n" % (
            indent + admonition,
            util.wrap(text, width=width, initindent=defindent, hangindent=defindent),
        )
    if block["type"] == "margin":
        return "\n"
    if block["type"] == "literal":
        indent += "  "
        return indent + ("\n" + indent).join(block["lines"]) + "\n"
    if block["type"] == "section":
        underline = encoding.colwidth(block["lines"][0]) * block["underline"]
        return "%s%s\n%s%s\n" % (indent, block["lines"][0], indent, underline)
    if block["type"] == "table":
        table = block["table"]
        # compute column widths
        widths = [max([encoding.colwidth(e) for e in c]) for c in zip(*table)]
        text = ""
        span = sum(widths) + len(widths) - 1
        indent = " " * block["indent"]
        hang = " " * (len(indent) + span - widths[-1])

        for row in table:
            l = []
            for w, v in zip(widths, row):
                pad = " " * (w - encoding.colwidth(v))
                l.append(v + pad)
            l = " ".join(l)
            l = util.wrap(l, width=width, initindent=indent, hangindent=hang)
            if not text and block["header"]:
                text = l + "\n" + indent + "-" * (min(width, span)) + "\n"
            else:
                text += l + "\n"
        return text
    if block["type"] == "definition":
        term = indent + block["lines"][0]
        hang = len(block["lines"][-1]) - len(block["lines"][-1].lstrip())
        defindent = indent + hang * " "
        text = " ".join(map(str.strip, block["lines"][1:]))
        return "%s\n%s\n" % (
            term,
            util.wrap(text, width=width, initindent=defindent, hangindent=defindent),
        )
    subindent = indent
    if block["type"] == "bullet":
        if block["lines"][0].startswith("| "):
            # Remove bullet for line blocks and add no extra
            # indentation.
            block["lines"][0] = block["lines"][0][2:]
        else:
            m = _bulletre.match(block["lines"][0])
            subindent = indent + m.end() * " "
    elif block["type"] == "field":
        key = block["key"]
        subindent = indent + _fieldwidth * " "
        if len(key) + 2 > _fieldwidth:
            # key too large, use full line width
            key = key.ljust(width)
        else:
            # key fits within field width
            key = key.ljust(_fieldwidth)
        block["lines"][0] = key + block["lines"][0]
    elif block["type"] == "option":
        return formatoption(block, width)

    text = " ".join(map(str.strip, block["lines"]))
    return util.wrap(text, width=width, initindent=indent, hangindent=subindent) + "\n"


def formatmd(blocks):
    out = []
    headernest = ""

    # We generally ignore indents because markdown does not like them, however
    # we do want to preserve the indentation of lists. The first bullet point
    # in a list will be translated to a 0 indent, and we will adjust each
    # subsequent indent relative to this first one.
    # We keep a stack of the translated indentation levels for lists here.
    indentation_translation = []

    def translate_indent(indent: int) -> int:
        nonlocal indentation_translation
        if len(indentation_translation) == 0:
            indentation_translation.append((indent, 0))
            return 0

        while len(indentation_translation) > 0:
            (last_raw_indent, last_real_indent) = indentation_translation[-1]
            # same level of indent
            if last_raw_indent == indent:
                return last_real_indent

            # We have a subindent
            if last_raw_indent < indent:
                new_indent = indent - last_raw_indent + last_real_indent
                indentation_translation.append((indent, new_indent))
                return new_indent

            # we broke out of a subindent, pop and find the parent/sibling level
            # of indent
            indentation_translation.pop()

        # we broke out to the top level
        indentation_translation.append((indent, 0))
        return 0

    for pos, b in enumerate(blocks):
        btype = b["type"]
        lines = b["lines"]

        if btype != "bullet":
            # new paragraphs of markdown bullets need to restart their indents
            indentation_translation = []

        if btype == "paragraph":
            import html

            text = "\n".join(map(html.escape, lines))
            out.append(f"\n{text}\n")
        elif btype == "literal":
            text = "\n".join(lines)
            out.append(f"\n```\n{text}\n```\n")
        elif btype == "section":
            i = b["underline"]
            if i not in headernest:
                headernest += i
            level = headernest.index(i) + 1
            header_value = "#" * level
            text = lines[0]
            out.append(f"\n{header_value} {text}\n")
        elif btype == "table":
            table = b["table"]
            if len(table):
                out.append("\n")
                tableheader = ("| " * len(table[0])) + "|\n"
                tablemiddle = ("| - " * len(table[0])) + "|\n"
                out.append(tableheader)
                out.append(tablemiddle)
            for row in table:
                for v in row:
                    out.append("| ")
                    out.append(v)
                    out.append(" ")
                out.append("|\n")
        elif btype == "definition":
            term = lines[0]
            text = "\n> ".join(map(str.strip, lines[1:]))
            out.append(f"\n{term}\n> {text}\n")
        elif btype == "bullet":
            out.append("\n")
            indent = " " * translate_indent(b["indent"])
            out.extend([f"{indent}{line}" for line in lines])
            out.append("\n")
        elif btype == "field":
            key = b["key"]
            text = " ".join(map(str.strip, lines))
            out.append(f"\n{key}\n{text}\n")
        elif btype == "option":
            opt = b["optstr"]
            desc = " ".join(map(str.strip, lines))
            out.append(f"\n{opt}\n{desc}\n")
        elif btype == "admonition":
            # TODO: at some point we should figure out how to convert these to
            # markdown well. The only text included in the admonition block
            # itself is the label like "note" or "warn" and the actual contents
            # often comes in a later block. The rst indicates the text is part
            # of the admonition with indentation. But we generally are ignoring
            # indentation here because markdown doesn't do indentation well.
            # for now we just skip the label as all existing admonitions make
            # sense with out it.
            pass
        elif btype == "margin":
            # markdown and indentation don't go well, so we just skip these
            # blocks
            pass

    return "".join(out)


def formathtml(blocks):
    """Format RST blocks as HTML"""

    out = []
    headernest = ""
    listnest = []

    def escape(s):
        return url.escape(s, True)

    def openlist(start, level):
        if not listnest or listnest[-1][0] != start:
            listnest.append((start, level))
            out.append("<%s>\n" % start)

    blocks = [b for b in blocks if b["type"] != "margin"]

    for pos, b in enumerate(blocks):
        btype = b["type"]
        level = b["indent"]
        lines = b["lines"]

        if btype == "admonition":
            admonition = escape(_admonitiontitles[b["admonitiontitle"]])
            text = escape(" ".join(map(str.strip, lines)))
            out.append("<p>\n<b>%s</b> %s\n</p>\n" % (admonition, text))
        elif btype == "paragraph":
            out.append("<p>\n%s\n</p>\n" % escape("\n".join(lines)))
        elif btype == "margin":
            pass
        elif btype == "literal":
            out.append("<pre>\n%s\n</pre>\n" % escape("\n".join(lines)))
        elif btype == "section":
            i = b["underline"]
            if i not in headernest:
                headernest += i
            level = headernest.index(i) + 1
            out.append("<h%d>%s</h%d>\n" % (level, escape(lines[0]), level))
        elif btype == "table":
            table = b["table"]
            out.append("<table>\n")
            for row in table:
                out.append("<tr>")
                for v in row:
                    out.append("<td>")
                    out.append(escape(v))
                    out.append("</td>")
                    out.append("\n")
                out.pop()
                out.append("</tr>\n")
            out.append("</table>\n")
        elif btype == "definition":
            openlist("dl", level)
            term = escape(lines[0])
            text = escape(" ".join(map(str.strip, lines[1:])))
            out.append(" <dt>%s\n <dd>%s\n" % (term, text))
        elif btype == "bullet":
            bullet, head = lines[0].split(" ", 1)
            if bullet in ("*", "-"):
                openlist("ul", level)
            else:
                openlist("ol", level)
            out.append(" <li> %s\n" % escape(" ".join([head] + lines[1:])))
        elif btype == "field":
            openlist("dl", level)
            key = escape(b["key"])
            text = escape(" ".join(map(str.strip, lines)))
            out.append(" <dt>%s\n <dd>%s\n" % (key, text))
        elif btype == "option":
            openlist("dl", level)
            opt = escape(b["optstr"])
            desc = escape(" ".join(map(str.strip, lines)))
            out.append(" <dt>%s\n <dd>%s\n" % (opt, desc))

        # close lists if indent level of next block is lower
        if listnest:
            start, level = listnest[-1]
            if pos == len(blocks) - 1:
                out.append("</%s>\n" % start)
                listnest.pop()
            else:
                nb = blocks[pos + 1]
                ni = nb["indent"]
                if ni < level or (
                    ni == level and nb["type"] not in "definition bullet field option"
                ):
                    out.append("</%s>\n" % start)
                    listnest.pop()

    return "".join(out)


def parse(text, indent=0, keep=None, admonitions=None, style="plain"):
    """Parse text into a list of blocks"""
    pruned = []
    blocks = findblocks(text)
    for b in blocks:
        b["indent"] += indent
    blocks = findliteralblocks(blocks)
    blocks = findtables(blocks)
    blocks, pruned = prunecontainers(blocks, keep or [])
    blocks = findsections(blocks)
    blocks = inlineliterals(blocks, style=style)
    blocks = hgrole(blocks, style=style)
    blocks = splitparagraphs(blocks)
    blocks = updatefieldlists(blocks)
    blocks = updateoptionlists(blocks)
    blocks = findadmonitions(blocks, admonitions=admonitions)
    blocks = addmargins(blocks)
    blocks = prunecomments(blocks)
    return blocks, pruned


def formatblocks(blocks, width):
    text = "".join(formatblock(b, width) for b in blocks)
    return text


def format(text, width=80, indent=0, keep=None, style="plain", section=None):
    """Parse and format the text according to width."""
    blocks, pruned = parse(text, indent, keep or [], style=style)
    parents = []
    if section:
        sections = getsections(blocks)
        blocks = []
        i = 0
        lastparents = []
        synthetic = []
        collapse = True
        while i < len(sections):
            name, nest, b = sections[i]
            del parents[nest:]
            parents.append(i)
            if name == section:
                if lastparents != parents:
                    llen = len(lastparents)
                    plen = len(parents)
                    if llen and llen != plen:
                        collapse = False
                    s = []
                    for j in range(3, plen - 1):
                        parent = parents[j]
                        if j >= llen or lastparents[j] != parent:
                            s.append(len(blocks))
                            sec = sections[parent][2]
                            blocks.append(sec[0])
                            blocks.append(sec[-1])
                    if s:
                        synthetic.append(s)

                lastparents = parents[:]
                blocks.extend(b)

                ## Also show all subnested sections
                while i + 1 < len(sections) and sections[i + 1][1] > nest:
                    i += 1
                    blocks.extend(sections[i][2])
            i += 1
        if collapse:
            synthetic.reverse()
            for s in synthetic:
                path = [blocks[syn]["lines"][0] for syn in s]
                real = s[-1] + 2
                realline = blocks[real]["lines"]
                realline[0] = '"%s"' % ".".join(path + [realline[0]]).replace('"', "")
                del blocks[s[0] : real]

    if style == "html":
        text = formathtml(blocks)
    elif style == "md":
        text = formatmd(blocks)
    else:
        text = "".join(formatblock(b, width) for b in blocks)
    if keep is None:
        return text
    else:
        return text, pruned


def getsections(blocks):
    """return a list of (section name, nesting level, blocks) tuples"""
    nest = ""
    level = 0
    secs = []

    def getname(b):
        if b["type"] == "field":
            x = b["key"]
        else:
            x = b["lines"][0]
        x = x.lower().strip('"')
        if "(" in x:
            x = x.split("(")[0]
        return x

    for b in blocks:
        if b["type"] == "section":
            i = b["underline"]
            if i not in nest:
                nest += i
            level = nest.index(i) + 1
            nest = nest[:level]
            secs.append((getname(b), level, [b]))
        elif b["type"] in ("definition", "field"):
            i = " "
            if i not in nest:
                nest += i
            level = nest.index(i) + 1
            nest = nest[:level]
            for i in range(1, len(secs) + 1):
                sec = secs[-i]
                if sec[1] < level:
                    break
                siblings = [a for a in sec[2] if a["type"] == "definition"]
                if siblings:
                    siblingindent = siblings[-1]["indent"]
                    indent = b["indent"]
                    if siblingindent < indent:
                        level += 1
                        break
                    elif siblingindent == indent:
                        level = sec[1]
                        break
            secs.append((getname(b), level, [b]))
        else:
            if not secs:
                # add an initial empty section
                secs = [("", 0, [])]
            if b["type"] != "margin":
                pointer = 1
                bindent = b["indent"]
                while pointer < len(secs):
                    section = secs[-pointer][2][0]
                    if section["type"] != "margin":
                        sindent = section["indent"]
                        if len(section["lines"]) > 1:
                            sindent += len(section["lines"][1]) - len(
                                section["lines"][1].lstrip(" ")
                            )
                        if bindent >= sindent:
                            break
                    pointer += 1
                if pointer > 1:
                    blevel = secs[-pointer][1]
                    if section["type"] != b["type"]:
                        blevel += 1
                    secs.append(("", blevel, []))
            secs[-1][2].append(b)
    return secs


def decorateblocks(blocks, width):
    """generate a list of (section name, line text) pairs for search"""
    lines = []
    for s in getsections(blocks):
        section = s[0]
        text = formatblocks(s[2], width)
        lines.append([(section, l) for l in text.splitlines(True)])
    return lines


def maketable(data, indent=0, header=False):
    """Generate an RST table for the given table data as a list of lines"""

    widths = [max(encoding.colwidth(e) for e in c) for c in zip(*data)]
    indent = " " * indent
    div = indent + " ".join("=" * w for w in widths) + "\n"

    out = [div]
    for row in data:
        l = []
        for w, v in zip(widths, row):
            if "\n" in v:
                # only remove line breaks and indentation, long lines are
                # handled by the next tool
                v = " ".join(e.lstrip() for e in v.split("\n"))
            pad = " " * (w - encoding.colwidth(v))
            l.append(v + pad)
        out.append(indent + " ".join(l) + "\n")
    if header and len(data) > 1:
        out.insert(2, div)
    out.append(div)
    return out
