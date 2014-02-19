from pprint import pprint
from mercurial import minirst

def debugformat(text, form, **kwargs):
    if form == 'html':
        print "html format:"
        out = minirst.format(text, style=form, **kwargs)
    else:
        print "%d column format:" % form
        out = minirst.format(text, width=form, **kwargs)

    print "-" * 70
    if type(out) == tuple:
        print out[0][:-1]
        print "-" * 70
        pprint(out[1])
    else:
        print out[:-1]
    print "-" * 70
    print

def debugformats(title, text, **kwargs):
    print "== %s ==" % title
    debugformat(text, 60, **kwargs)
    debugformat(text, 30, **kwargs)
    debugformat(text, 'html', **kwargs)

paragraphs = """
This is some text in the first paragraph.

  A small indented paragraph.
  It is followed by some lines
  containing random whitespace.
 \n  \n   \nThe third and final paragraph.
"""

debugformats('paragraphs', paragraphs)

definitions = """
A Term
  Definition. The indented
  lines make up the definition.
Another Term
  Another definition. The final line in the
   definition determines the indentation, so
    this will be indented with four spaces.

  A Nested/Indented Term
    Definition.
"""

debugformats('definitions', definitions)

literals = r"""
The fully minimized form is the most
convenient form::

  Hello
    literal
      world

In the partially minimized form a paragraph
simply ends with space-double-colon. ::

  ////////////////////////////////////////
  long un-wrapped line in a literal block
  \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\

::

  This literal block is started with '::',
    the so-called expanded form. The paragraph
      with '::' disappears in the final output.
"""

debugformats('literals', literals)

lists = """
- This is the first list item.

  Second paragraph in the first list item.

- List items need not be separated
  by a blank line.
- And will be rendered without
  one in any case.

We can have indented lists:

  - This is an indented list item

  - Another indented list item::

      - A literal block in the middle
            of an indented list.

      (The above is not a list item since we are in the literal block.)

::

  Literal block with no indentation (apart from
  the two spaces added to all literal blocks).

1. This is an enumerated list (first item).
2. Continuing with the second item.

(1) foo
(2) bar

1) Another
2) List

Line blocks are also a form of list:

| This is the first line.
  The line continues here.
| This is the second line.
"""

debugformats('lists', lists)

options = """
There is support for simple option lists,
but only with long options:

-X, --exclude  filter  an option with a short and long option with an argument
-I, --include          an option with both a short option and a long option
--all                  Output all.
--both                 Output both (this description is
                       quite long).
--long                 Output all day long.

--par                 This option has two paragraphs in its description.
                      This is the first.

                      This is the second.  Blank lines may be omitted between
                      options (as above) or left in (as here).


The next paragraph looks like an option list, but lacks the two-space
marker after the option. It is treated as a normal paragraph:

--foo bar baz
"""

debugformats('options', options)

fields = """
:a: First item.
:ab: Second item. Indentation and wrapping
     is handled automatically.

Next list:

:small: The larger key below triggers full indentation here.
:much too large: This key is big enough to get its own line.
"""

debugformats('fields', fields)

containers = """
Normal output.

.. container:: debug

   Initial debug output.

.. container:: verbose

   Verbose output.

   .. container:: debug

      Debug output.
"""

debugformats('containers (normal)', containers)
debugformats('containers (verbose)', containers, keep=['verbose'])
debugformats('containers (debug)', containers, keep=['debug'])
debugformats('containers (verbose debug)', containers,
            keep=['verbose', 'debug'])

roles = """Please see :hg:`add`."""
debugformats('roles', roles)


sections = """
Title
=====

Section
-------

Subsection
''''''''''

Markup: ``foo`` and :hg:`help`
------------------------------
"""
debugformats('sections', sections)


admonitions = """
.. note::

   This is a note

   - Bullet 1
   - Bullet 2

   .. warning:: This is a warning Second
      input line of warning

.. danger::
   This is danger
"""

debugformats('admonitions', admonitions)

comments = """
Some text.

.. A comment

   .. An indented comment

   Some indented text.

..

Empty comment above
"""

debugformats('comments', comments)


data = [['a', 'b', 'c'],
         ['1', '2', '3'],
         ['foo', 'bar', 'baz this list is very very very long man']]

rst = minirst.maketable(data, 2, True)
table = ''.join(rst)

print table

debugformats('table', table)

data = [['s', 'long', 'line\ngoes on here'],
        ['', 'xy', 'tried to fix here\n        by indenting']]

rst = minirst.maketable(data, 1, False)
table = ''.join(rst)

print table

debugformats('table+nl', table)

