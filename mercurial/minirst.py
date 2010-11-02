# minirst.py - minimal reStructuredText parser
#
# Copyright 2009, 2010 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""simplified reStructuredText parser.

This parser knows just enough about reStructuredText to parse the
Mercurial docstrings.

It cheats in a major way: nested blocks are not really nested. They
are just indented blocks that look like they are nested. This relies
on the user to keep the right indentation for the blocks.

Remember to update http://mercurial.selenic.com/wiki/HelpStyleGuide
when adding support for new constructs.
"""

import re, sys
import util, encoding
from i18n import _


def replace(text, substs):
    utext = text.decode(encoding.encoding)
    for f, t in substs:
        utext = utext.replace(f, t)
    return utext.encode(encoding.encoding)


_blockre = re.compile(r"\n(?:\s*\n)+")

def findblocks(text):
    """Find continuous blocks of lines in text.

    Returns a list of dictionaries representing the blocks. Each block
    has an 'indent' field and a 'lines' field.
    """
    blocks = []
    for b in _blockre.split(text.strip()):
        lines = b.splitlines()
        indent = min((len(l) - len(l.lstrip())) for l in lines)
        lines = [l[indent:] for l in lines]
        blocks.append(dict(indent=indent, lines=lines))
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
        blocks[i]['type'] = 'paragraph'
        if blocks[i]['lines'][-1].endswith('::') and i + 1 < len(blocks):
            indent = blocks[i]['indent']
            adjustment = blocks[i + 1]['indent'] - indent

            if blocks[i]['lines'] == ['::']:
                # Expanded form: remove block
                del blocks[i]
                i -= 1
            elif blocks[i]['lines'][-1].endswith(' ::'):
                # Partially minimized form: remove space and both
                # colons.
                blocks[i]['lines'][-1] = blocks[i]['lines'][-1][:-3]
            else:
                # Fully minimized form: remove just one colon.
                blocks[i]['lines'][-1] = blocks[i]['lines'][-1][:-1]

            # List items are formatted with a hanging indent. We must
            # correct for this here while we still have the original
            # information on the indentation of the subsequent literal
            # blocks available.
            m = _bulletre.match(blocks[i]['lines'][0])
            if m:
                indent += m.end()
                adjustment -= m.end()

            # Mark the following indented blocks.
            while i + 1 < len(blocks) and blocks[i + 1]['indent'] > indent:
                blocks[i + 1]['type'] = 'literal'
                blocks[i + 1]['indent'] -= adjustment
                i += 1
        i += 1
    return blocks

_bulletre = re.compile(r'(-|[0-9A-Za-z]+\.|\(?[0-9A-Za-z]+\)|\|) ')
_optionre = re.compile(r'^(-([a-zA-Z0-9]), )?(--[a-z0-9-]+)'
                       r'((.*)  +)(.*)$')
_fieldre = re.compile(r':(?![: ])([^:]*)(?<! ):[ ]+(.*)')
_definitionre = re.compile(r'[^ ]')

def splitparagraphs(blocks):
    """Split paragraphs into lists."""
    # Tuples with (list type, item regexp, single line items?). Order
    # matters: definition lists has the least specific regexp and must
    # come last.
    listtypes = [('bullet', _bulletre, True),
                 ('option', _optionre, True),
                 ('field', _fieldre, True),
                 ('definition', _definitionre, False)]

    def match(lines, i, itemre, singleline):
        """Does itemre match an item at line i?

        A list item can be followed by an idented line or another list
        item (but only if singleline is True).
        """
        line1 = lines[i]
        line2 = i + 1 < len(lines) and lines[i + 1] or ''
        if not itemre.match(line1):
            return False
        if singleline:
            return line2 == '' or line2[0] == ' ' or itemre.match(line2)
        else:
            return line2.startswith(' ')

    i = 0
    while i < len(blocks):
        if blocks[i]['type'] == 'paragraph':
            lines = blocks[i]['lines']
            for type, itemre, singleline in listtypes:
                if match(lines, 0, itemre, singleline):
                    items = []
                    for j, line in enumerate(lines):
                        if match(lines, j, itemre, singleline):
                            items.append(dict(type=type, lines=[],
                                              indent=blocks[i]['indent']))
                        items[-1]['lines'].append(line)
                    blocks[i:i + 1] = items
                    break
        i += 1
    return blocks


_fieldwidth = 12

def updatefieldlists(blocks):
    """Find key and maximum key width for field lists."""
    i = 0
    while i < len(blocks):
        if blocks[i]['type'] != 'field':
            i += 1
            continue

        keywidth = 0
        j = i
        while j < len(blocks) and blocks[j]['type'] == 'field':
            m = _fieldre.match(blocks[j]['lines'][0])
            key, rest = m.groups()
            blocks[j]['lines'][0] = rest
            blocks[j]['key'] = key
            keywidth = max(keywidth, len(key))
            j += 1

        for block in blocks[i:j]:
            block['keywidth'] = keywidth
        i = j + 1

    return blocks


def updateoptionlists(blocks):
    i = 0
    while i < len(blocks):
        if blocks[i]['type'] != 'option':
            i += 1
            continue

        optstrwidth = 0
        j = i
        while j < len(blocks) and blocks[j]['type'] == 'option':
            m = _optionre.match(blocks[j]['lines'][0])

            shortoption = m.group(2)
            group3 = m.group(3)
            longoption = group3[2:].strip()
            desc = m.group(6).strip()
            longoptionarg = m.group(5).strip()
            blocks[j]['lines'][0] = desc

            noshortop = ''
            if not shortoption:
                noshortop = '   '

            opt = "%s%s" %   (shortoption and "-%s " % shortoption or '',
                            ("%s--%s %s") % (noshortop, longoption,
                                             longoptionarg))
            opt = opt.rstrip()
            blocks[j]['optstr'] = opt
            optstrwidth = max(optstrwidth, encoding.colwidth(opt))
            j += 1

        for block in blocks[i:j]:
            block['optstrwidth'] = optstrwidth
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
        if (blocks[i]['type'] == 'paragraph' and
            blocks[i]['lines'][0].startswith('.. container::')):
            indent = blocks[i]['indent']
            adjustment = blocks[i + 1]['indent'] - indent
            containertype = blocks[i]['lines'][0][15:]
            prune = containertype not in keep
            if prune:
                pruned.append(containertype)

            # Always delete "..container:: type" block
            del blocks[i]
            j = i
            while j < len(blocks) and blocks[j]['indent'] > indent:
                if prune:
                    del blocks[j]
                    i -= 1 # adjust outer index
                else:
                    blocks[j]['indent'] -= adjustment
                    j += 1
        i += 1
    return blocks, pruned


_sectionre = re.compile(r"""^([-=`:.'"~^_*+#])\1+$""")

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
        if (block['type'] == 'paragraph' and
            len(block['lines']) == 2 and
            encoding.colwidth(block['lines'][0]) == len(block['lines'][1]) and
            _sectionre.match(block['lines'][1])):
            block['underline'] = block['lines'][1][0]
            block['type'] = 'section'
            del block['lines'][1]
    return blocks


def inlineliterals(blocks):
    substs = [('``', '"')]
    for b in blocks:
        if b['type'] in ('paragraph', 'section'):
            b['lines'] = [replace(l, substs) for l in b['lines']]
    return blocks


def hgrole(blocks):
    substs = [(':hg:`', '"hg '), ('`', '"')]
    for b in blocks:
        if b['type'] in ('paragraph', 'section'):
            # Turn :hg:`command` into "hg command". This also works
            # when there is a line break in the command and relies on
            # the fact that we have no stray back-quotes in the input
            # (run the blocks through inlineliterals first).
            b['lines'] = [replace(l, substs) for l in b['lines']]
    return blocks


def addmargins(blocks):
    """Adds empty blocks for vertical spacing.

    This groups bullets, options, and definitions together with no vertical
    space between them, and adds an empty block between all other blocks.
    """
    i = 1
    while i < len(blocks):
        if (blocks[i]['type'] == blocks[i - 1]['type'] and
            blocks[i]['type'] in ('bullet', 'option', 'field')):
            i += 1
        else:
            blocks.insert(i, dict(lines=[''], indent=0, type='margin'))
            i += 2
    return blocks

def prunecomments(blocks):
    """Remove comments."""
    i = 0
    while i < len(blocks):
        b = blocks[i]
        if b['type'] == 'paragraph' and (b['lines'][0].startswith('.. ') or
                                         b['lines'] == ['..']):
            del blocks[i]
            if i < len(blocks) and blocks[i]['type'] == 'margin':
                del blocks[i]
        else:
            i += 1
    return blocks

_admonitionre = re.compile(r"\.\. (admonition|attention|caution|danger|"
                           r"error|hint|important|note|tip|warning)::",
                           flags=re.IGNORECASE)

def findadmonitions(blocks):
    """
    Makes the type of the block an admonition block if
    the first line is an admonition directive
    """
    i = 0
    while i < len(blocks):
        m = _admonitionre.match(blocks[i]['lines'][0])
        if m:
            blocks[i]['type'] = 'admonition'
            admonitiontitle = blocks[i]['lines'][0][3:m.end() - 2].lower()

            firstline = blocks[i]['lines'][0][m.end() + 1:]
            if firstline:
                blocks[i]['lines'].insert(1, '   ' + firstline)

            blocks[i]['admonitiontitle'] = admonitiontitle
            del blocks[i]['lines'][0]
        i = i + 1
    return blocks

_admonitiontitles = {'attention': _('Attention:'),
                     'caution': _('Caution:'),
                     'danger': _('!Danger!')  ,
                     'error': _('Error:'),
                     'hint': _('Hint:'),
                     'important': _('Important:'),
                     'note': _('Note:'),
                     'tip': _('Tip:'),
                     'warning': _('Warning!')}

def formatoption(block, width):
    desc = ' '.join(map(str.strip, block['lines']))
    colwidth = encoding.colwidth(block['optstr'])
    usablewidth = width - 1
    hanging = block['optstrwidth']
    initindent = '%s%s  ' % (block['optstr'], ' ' * ((hanging - colwidth)))
    hangindent = ' ' * (encoding.colwidth(initindent) + 1)
    return ' %s' % (util.wrap(desc, usablewidth,
                                           initindent=initindent,
                                           hangindent=hangindent))

def formatblock(block, width):
    """Format a block according to width."""
    if width <= 0:
        width = 78
    indent = ' ' * block['indent']
    if block['type'] == 'admonition':
        admonition = _admonitiontitles[block['admonitiontitle']]
        hang = len(block['lines'][-1]) - len(block['lines'][-1].lstrip())

        defindent = indent + hang * ' '
        text = ' '.join(map(str.strip, block['lines']))
        return '%s\n%s' % (indent + admonition, util.wrap(text, width=width,
                                           initindent=defindent,
                                           hangindent=defindent))
    if block['type'] == 'margin':
        return ''
    if block['type'] == 'literal':
        indent += '  '
        return indent + ('\n' + indent).join(block['lines'])
    if block['type'] == 'section':
        underline = encoding.colwidth(block['lines'][0]) * block['underline']
        return "%s%s\n%s%s" % (indent, block['lines'][0],indent, underline)
    if block['type'] == 'definition':
        term = indent + block['lines'][0]
        hang = len(block['lines'][-1]) - len(block['lines'][-1].lstrip())
        defindent = indent + hang * ' '
        text = ' '.join(map(str.strip, block['lines'][1:]))
        return '%s\n%s' % (term, util.wrap(text, width=width,
                                           initindent=defindent,
                                           hangindent=defindent))
    subindent = indent
    if block['type'] == 'bullet':
        if block['lines'][0].startswith('| '):
            # Remove bullet for line blocks and add no extra
            # indention.
            block['lines'][0] = block['lines'][0][2:]
        else:
            m = _bulletre.match(block['lines'][0])
            subindent = indent + m.end() * ' '
    elif block['type'] == 'field':
        keywidth = block['keywidth']
        key = block['key']

        subindent = indent + _fieldwidth * ' '
        if len(key) + 2 > _fieldwidth:
            # key too large, use full line width
            key = key.ljust(width)
        elif keywidth + 2 < _fieldwidth:
            # all keys are small, add only two spaces
            key = key.ljust(keywidth + 2)
            subindent = indent + (keywidth + 2) * ' '
        else:
            # mixed sizes, use fieldwidth for this one
            key = key.ljust(_fieldwidth)
        block['lines'][0] = key + block['lines'][0]
    elif block['type'] == 'option':
        return formatoption(block, width)

    text = ' '.join(map(str.strip, block['lines']))
    return util.wrap(text, width=width,
                     initindent=indent,
                     hangindent=subindent)


def format(text, width, indent=0, keep=None):
    """Parse and format the text according to width."""
    blocks = findblocks(text)
    for b in blocks:
        b['indent'] += indent
    blocks = findliteralblocks(blocks)
    blocks, pruned = prunecontainers(blocks, keep or [])
    blocks = findsections(blocks)
    blocks = inlineliterals(blocks)
    blocks = hgrole(blocks)
    blocks = splitparagraphs(blocks)
    blocks = updatefieldlists(blocks)
    blocks = updateoptionlists(blocks)
    blocks = addmargins(blocks)
    blocks = prunecomments(blocks)
    blocks = findadmonitions(blocks)
    text = '\n'.join(formatblock(b, width) for b in blocks)
    if keep is None:
        return text
    else:
        return text, pruned


if __name__ == "__main__":
    from pprint import pprint

    def debug(func, *args):
        blocks = func(*args)
        print "*** after %s:" % func.__name__
        pprint(blocks)
        print
        return blocks

    text = open(sys.argv[1]).read()
    blocks = debug(findblocks, text)
    blocks = debug(findliteralblocks, blocks)
    blocks, pruned = debug(prunecontainers, blocks, sys.argv[2:])
    blocks = debug(inlineliterals, blocks)
    blocks = debug(splitparagraphs, blocks)
    blocks = debug(updatefieldlists, blocks)
    blocks = debug(updateoptionlists, blocks)
    blocks = debug(findsections, blocks)
    blocks = debug(addmargins, blocks)
    blocks = debug(prunecomments, blocks)
    blocks = debug(findadmonitions, blocks)
    print '\n'.join(formatblock(b, 30) for b in blocks)
