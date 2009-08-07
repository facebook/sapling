#!/usr/bin/env python

from mercurial import minirst

def debugformat(title, text, width):
    print "%s formatted to fit within %d characters:" % (title, width)
    print "-" * 70
    print minirst.format(text, width)
    print "-" * 70
    print

paragraphs = """
This is some text in the first paragraph.

  An indented paragraph
  with just two lines.


The third paragraph. It is followed by some
random lines with spurious spaces.
 
  
   
  
 
No indention
 here, despite
the uneven left
 margin.

      Only the
    left-most line
  (this line!)
    is significant
      for the indentation

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

  Literal block with no indentation.
"""

debugformat('lists', lists, 60)
debugformat('lists', lists, 30)


options = """
There is support for simple option lists,
but only with long options:

--all      Output all.
--both     Output both (this description is
           quite long).
--long     Output all day long.

--par      This option has two paragraphs in its description.
           This is the first.

           This is the second.  Blank lines may be omitted between
           options (as above) or left in (as here).

The next paragraph looks like an option list, but lacks the two-space
marker after the option. It is treated as a normal paragraph:

--foo bar baz
"""

debugformat('options', options, 60)
debugformat('options', options, 30)
