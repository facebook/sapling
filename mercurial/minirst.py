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
            if blocks[i]['lines'][0].startswith('- '):
                indent += 2
                adjustment -= 2

            # Mark the following indented blocks.
            while i+1 < len(blocks) and blocks[i+1]['indent'] > indent:
                blocks[i+1]['type'] = 'literal'
                blocks[i+1]['indent'] -= adjustment
                i += 1
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


def findbulletlists(blocks):
    """Finds bullet lists.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    i = 0
    while i < len(blocks):
        # Searching for a paragraph that looks like this:
        #
        # +------+-----------------------+
        # | "- " | list item             |
        # +------| (body elements)+      |
        #        +-----------------------+
        if (blocks[i]['type'] == 'paragraph' and
            blocks[i]['lines'][0].startswith('- ')):
            items = []
            for line in blocks[i]['lines']:
                if line.startswith('- '):
                    items.append(dict(type='bullet', lines=[],
                                      indent=blocks[i]['indent']))
                    line = line[2:]
                items[-1]['lines'].append(line)
            blocks[i:i+1] = items
            i += len(items) - 1
        i += 1
    return blocks


_optionre = re.compile(r'^(--[a-z-]+)((?:[ =][a-zA-Z][\w-]*)?  +)(.*)$')
def findoptionlists(blocks):
    """Finds option lists.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    i = 0
    while i < len(blocks):
        # Searching for a paragraph that looks like this:
        #
        # +----------------------------+-------------+
        # | "--" option "  "           | description |
        # +-------+--------------------+             |
        #         | (body elements)+                 |
        #         +----------------------------------+
        if (blocks[i]['type'] == 'paragraph' and
            _optionre.match(blocks[i]['lines'][0])):
            options = []
            for line in blocks[i]['lines']:
                m = _optionre.match(line)
                if m:
                    option, arg, rest = m.groups()
                    width = len(option) + len(arg)
                    options.append(dict(type='option', lines=[],
                                        indent=blocks[i]['indent'],
                                        width=width))
                options[-1]['lines'].append(line)
            blocks[i:i+1] = options
            i += len(options) - 1
        i += 1
    return blocks


_fieldre = re.compile(r':(?![: ])([^:]*)(?<! ):( +)(.*)')
def findfieldlists(blocks):
    """Finds fields lists.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    i = 0
    while i < len(blocks):
        # Searching for a paragraph that looks like this:
        #
        #
        # +--------------------+----------------------+
        # | ":" field name ":" | field body           |
        # +-------+------------+                      |
        #         | (body elements)+                  |
        #         +-----------------------------------+
        if (blocks[i]['type'] == 'paragraph' and
            _fieldre.match(blocks[i]['lines'][0])):
            indent = blocks[i]['indent']
            fields = []
            for line in blocks[i]['lines']:
                m = _fieldre.match(line)
                if m:
                    key, spaces, rest = m.groups()
                    width = 2 + len(key) + len(spaces)
                    fields.append(dict(type='field', lines=[],
                                       indent=indent, width=width))
                    # Turn ":foo: bar" into "foo   bar".
                    line = '%s  %s%s' % (key, spaces, rest)
                fields[-1]['lines'].append(line)
            blocks[i:i+1] = fields
            i += len(fields) - 1
        i += 1
    return blocks


def finddefinitionlists(blocks):
    """Finds definition lists.

    The blocks must have a 'type' field, i.e., they should have been
    run through findliteralblocks first.
    """
    i = 0
    while i < len(blocks):
        # Searching for a paragraph that looks like this:
        #
        # +----------------------------+
        # | term                       |
        # +--+-------------------------+--+
        #    | definition                 |
        #    | (body elements)+           |
        #    +----------------------------+
        if (blocks[i]['type'] == 'paragraph' and
            len(blocks[i]['lines']) > 1 and
            not blocks[i]['lines'][0].startswith('  ') and
            blocks[i]['lines'][1].startswith('  ')):
            definitions = []
            for line in blocks[i]['lines']:
                if not line.startswith('  '):
                    definitions.append(dict(type='definition', lines=[],
                                            indent=blocks[i]['indent']))
                definitions[-1]['lines'].append(line)
                definitions[-1]['hang'] = len(line) - len(line.lstrip())
            blocks[i:i+1] = definitions
            i += len(definitions) - 1
        i += 1
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
    indent = ' ' * block['indent']
    if block['type'] == 'margin':
        return ''
    elif block['type'] == 'literal':
        indent += '  '
        return indent + ('\n' + indent).join(block['lines'])
    elif block['type'] == 'section':
        return indent + ('\n' + indent).join(block['lines'])
    elif block['type'] == 'definition':
        term = indent + block['lines'][0]
        defindent = indent + block['hang'] * ' '
        text = ' '.join(map(str.strip, block['lines'][1:]))
        return "%s\n%s" % (term, textwrap.fill(text, width=width,
                                               initial_indent=defindent,
                                               subsequent_indent=defindent))
    else:
        initindent = subindent = indent
        text = ' '.join(map(str.strip, block['lines']))
        if block['type'] == 'bullet':
            initindent = indent + '- '
            subindent = indent + '  '
        elif block['type'] in ('option', 'field'):
            subindent = indent + block['width'] * ' '

        return textwrap.fill(text, width=width,
                             initial_indent=initindent,
                             subsequent_indent=subindent)


def format(text, width):
    """Parse and format the text according to width."""
    blocks = findblocks(text)
    blocks = findliteralblocks(blocks)
    blocks = findsections(blocks)
    blocks = findbulletlists(blocks)
    blocks = findoptionlists(blocks)
    blocks = findfieldlists(blocks)
    blocks = finddefinitionlists(blocks)
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
    blocks = debug(findsections, blocks)
    blocks = debug(findbulletlists, blocks)
    blocks = debug(findoptionlists, blocks)
    blocks = debug(findfieldlists, blocks)
    blocks = debug(finddefinitionlists, blocks)
    blocks = debug(addmargins, blocks)
    print '\n'.join(formatblock(b, 30) for b in blocks)
