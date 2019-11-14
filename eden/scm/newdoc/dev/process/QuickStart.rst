Quick Start
===========

Build
-----
Run ``make local``. Then ``hg`` from the project root can be executed.


Test
----
To test native components: ``cd lib && cargo test``.

To run integration tests (the main test suite): Build. Then run ``cd tests && ./run-tests.py``. 


Directory Layout
----------------

- ``mercurial``: core Python modules and pure C utilities
  - ``mercurial/cext``: C Python wrappers
  - ``mercurial/rust``: Rust Python wrappers
- ``lib``: Native components unrelated to Python
- ``exec``: Native standalone executable projects unrelated to Python
- ``hgext``: Extensions
  - ``hgext/extlib``: Python wrappers. Similar to ``mercurial/rust`` or ``mercurial/cext``, but for extensions.
- ``tests``: Mostly integration tests

.. note:: The above layout assumes that ``hg`` is mainly a Python program. That will change over time.


Coding Style
------------

tl;dr: Mimit the style of the existing code.

Python
~~~~~~

Use ``foobar`` naming, not ``foo_bar``, nor ``fooBar``. Use ``_foobar`` for private fields.

Use full English words for variable names. Except for well-know ones:
  - ``p1``, ``p2``: first and second parents
  - ``ctx``, ``fctx``: ``context.changectx`` and ``context.filectx`` instances
  - ``fp``, ``fd``: python file-like object, file descriptor
  - ``repo``, ``unfi``: repo, unfiltered repo instances

Rust
~~~~

Use `foo_bar`, like the rest of the Rust community.

Use `def foobar` for Python bindings used in the ``foobar``-style code base.

Try match the Rust community standard. Especially for documentation.

Commit
~~~~~~
Use ``[hg] topic: short summary without capital letters`` for commit title.


What's next
-----------

To add commands, revsets as an extension. See :doc:`WritingExtensions`.

To make the codebase more confident. See :doc:`WritingTests`.

To understand internal data structures and protocols. See (TODO).
