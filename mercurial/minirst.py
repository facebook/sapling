# minirst.py - minimal reStructuredText parser
#
# Copyright 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

"""simplified reStructuredText parser.

This parser knows just enough about reStructuredText to parse the
Mercurial docstrings.

It cheats in a major way: nested blocks are not really nested. They
are just indented blocks that look like they are nested. This relies
on the user to keep the right indentation for the blocks.

It only supports a small subset of reStructuredText:

- paragraphs

- definition lists (must use '  ' to indent definitions)

- lists (items must start with '-')

- field lists (colons cannot be escaped)

- literal blocks

- option lists (supports only long options without arguments)

- inline markup is not recognized at all.
"""

import re, sys, textwrap


def findblocks(text):
    """Find continuous blocks of lines in text.

    Returns a list of dictionaries representing the blocks. Each block
    has an 'indent' field and a 'lines' field.
    """
    blocks = [[]]
    lines = text.splitlines()
    for line in lines:
        if line.strip():
            blocks[-1].append(line)
        elif blocks[-1]:
            blocks.append([])
    if not blocks[-1]:
        del blocks[-1]

    for i, block in enumerate(blocks):
        indent = min((len(l) - len(l.lstrip())) for l in block)
        blocks[i] = dict(indent=indent, lines=[l[indent:] for l in block])
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
        if blocks[i]['lines'][-1].endswith('::') and i+1 < len(blocks):
            indent = blocks[i]['indent']
            adjustment = blocks[i+1]['indent'] - indent

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
            while i+1 < len(blocks) and blocks[i+1]['indent'] > indent:
                blocks[i+1]['type'] = 'literal'
                blocks[i+1]['indent'] -= adjustment
                i += 1
        i += 1
    return blocks

_bulletre = re.compile(r'(-|[0-9A-Za-z]+\.|\(?[0-9A-Za-z]+\)) ')
_optionre = re.compile(r'^(--[a-z-]+)((?:[ =][a-zA-Z][\w-]*)?  +)(.*)$')
_fieldre = re.compile(r':(?![: ])([^:]*)(?<! ):( +)(.*)')
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
        line2 = i+1 < len(lines) and lines[i+1] or ''
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
                    blocks[i:i+1] = items
                    break
        i += 1
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
        if (block['type'] == 'paragraph' and
            len(block['lines']) == 2 and
            block['lines'][1] == '-' * len(block['lines'][0])):
            block['type'] = 'section'
    return blocks


def inlineliterals(blocks):
    for b in blocks:
        if b['type'] == 'paragraph':
            b['lines'] = [l.replace('``', '"') for l in b['lines']]
    return blocks


def addmargins(blocks):
    """Adds empty blocks for vertical spacing.

    This groups bullets, options, and definitions together with no vertical
    space between them, and adds an empty block between all other blocks.
    """
    i = 1
    while i < len(blocks):
        if (blocks[i]['type'] == blocks[i-1]['type'] and
            blocks[i]['type'] in ('bullet', 'option', 'field', 'definition')):
            i += 1
        else:
            blocks.insert(i, dict(lines=[''], indent=0, type='margin'))
            i += 2
    return blocks


def formatblock(block, width):
    """Format a block according to width."""
    if width <= 0:
        width = 78
    indent = ' ' * block['indent']
    if block['type'] == 'margin':
        return ''
    if block['type'] == 'literal':
        indent += '  '
        return indent + ('\n' + indent).join(block['lines'])
    if block['type'] == 'section':
        return indent + ('\n' + indent).join(block['lines'])
    if block['type'] == 'definition':
        term = indent + block['lines'][0]
        hang = len(block['lines'][-1]) - len(block['lines'][-1].lstrip())
        defindent = indent + hang * ' '
        text = ' '.join(map(str.strip, block['lines'][1:]))
        return "%s\n%s" % (term, textwrap.fill(text, width=width,
                                               initial_indent=defindent,
                                               subsequent_indent=defindent))
    initindent = subindent = indent
    if block['type'] == 'bullet':
        m = _bulletre.match(block['lines'][0])
        if m:
            subindent = indent + m.end() * ' '
    elif block['type'] == 'field':
        m = _fieldre.match(block['lines'][0])
        if m:
            key, spaces, rest = m.groups()
            # Turn ":foo: bar" into "foo   bar".
            block['lines'][0] = '%s  %s%s' % (key, spaces, rest)
            subindent = indent + (2 + len(key) + len(spaces)) * ' '
    elif block['type'] == 'option':
        m = _optionre.match(block['lines'][0])
        if m:
            option, arg, rest = m.groups()
            subindent = indent + (len(option) + len(arg)) * ' '

    text = ' '.join(map(str.strip, block['lines']))
    return textwrap.fill(text, width=width,
                         initial_indent=initindent,
                         subsequent_indent=subindent)


def format(text, width, indent=0):
    """Parse and format the text according to width."""
    blocks = findblocks(text)
    for b in blocks:
        b['indent'] += indent
    blocks = findliteralblocks(blocks)
    blocks = inlineliterals(blocks)
    blocks = splitparagraphs(blocks)
    blocks = findsections(blocks)
    blocks = addmargins(blocks)
    return '\n'.join(formatblock(b, width) for b in blocks)


if __name__ == "__main__":
    from pprint import pprint

    def debug(func, blocks):
        blocks = func(blocks)
        print "*** after %s:" % func.__name__
        pprint(blocks)
        print
        return blocks

    text = open(sys.argv[1]).read()
    blocks = debug(findblocks, text)
    blocks = debug(findliteralblocks, blocks)
    blocks = debug(splitparagraphs, blocks)
    blocks = debug(findsections, blocks)
    blocks = debug(addmargins, blocks)
    print '\n'.join(formatblock(b, 30) for b in blocks)
