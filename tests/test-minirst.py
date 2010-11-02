from pprint import pprint
from mercurial import minirst

def debugformat(title, text, width, **kwargs):
    print "%s formatted to fit within %d characters:" % (title, width)
    print "-" * 70
    formatted = minirst.format(text, width, **kwargs)
    if type(formatted) == tuple:
        print formatted[0]
        print "-" * 70
        pprint(formatted[1])
    else:
        print formatted
    print "-" * 70
    print

paragraphs = """
This is some text in the first paragraph.

  A small indented paragraph.
  It is followed by some lines
  containing random whitespace.
 \n  \n   \nThe third and final paragraph.
"""

debugformat('paragraphs', paragraphs, 60)
debugformat('paragraphs', paragraphs, 30)


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

debugformat('definitions', definitions, 60)
debugformat('definitions', definitions, 30)


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

debugformat('literals', literals, 60)
debugformat('literals', literals, 30)


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

debugformat('lists', lists, 60)
debugformat('lists', lists, 30)


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

debugformat('options', options, 60)
debugformat('options', options, 30)


fields = """
:a: First item.
:ab: Second item. Indentation and wrapping
     is handled automatically.

Next list:

:small: The larger key below triggers full indentation here.
:much too large: This key is big enough to get its own line.
"""

debugformat('fields', fields, 60)
debugformat('fields', fields, 30)

containers = """
Normal output.

.. container:: debug

   Initial debug output.

.. container:: verbose

   Verbose output.

   .. container:: debug

      Debug output.
"""

debugformat('containers (normal)', containers, 60)
debugformat('containers (verbose)', containers, 60, keep=['verbose'])
debugformat('containers (debug)', containers, 60, keep=['debug'])
debugformat('containers (verbose debug)', containers, 60,
            keep=['verbose', 'debug'])

roles = """Please see :hg:`add`."""
debugformat('roles', roles, 60)


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
debugformat('sections', sections, 20)


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

debugformat('admonitions', admonitions, 30)

comments = """
Some text.

.. A comment

   .. An indented comment

   Some indented text.

..

Empty comment above
"""

debugformat('comments', comments, 30)
