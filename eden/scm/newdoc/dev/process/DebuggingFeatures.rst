Debugging Features
==================

Mercurial has a bunch of features for debugging problems that are useful for developers to know about.

See also: :doc:`DebuggingTests`

Global Options
--------------

``-v``, ``--verbose``
    show more verbose output

``--debug``
    show extended debugging output

``--traceback``
    show Python tracebacks that are otherwise hidden

``--profile``
    generate performance profiling information

``--debugger``
    drop into the built-in source-level debugger (more below)

Debug Commands
--------------

``debugcheckstate``
    validate the correctness of the current dirstate

``debugconfig``
    show combined config settings from all hgrc files

``debugdata``
    dump the contents of an data file revision

``debugindex``
    dump the contents of an index file

``debugindexdot``
    dump an index DAG as a .dot file

``debugrename``
    dump rename information

``debugstate``
    show the contents of the current dirstate

``debugwalk``
    show how files match on given patterns

To get a complete up-to-date list of all available debug commands use ``hg debugcomplete debug``:

::

   > hg debugcomplete debug
   debugancestor
   debugcheckstate
   debugcomplete
   debugconfig
   debugdata
   debugdate
   debugfsinfo
   debugindex
   debugindexdot
   debuginstall
   debugrawcommit
   debugrebuildstate
   debugrename
   debugsetparents
   debugstate
   debugwalk

Documentation for some debug commands is available through ``hg help``:

::

   > hg help debugstate
   hg debugstate

   show the contents of the current dirstate

   use "hg -v help debugstate" to show global options

Debugger
--------

Using the basic debugger
~~~~~~~~~~~~~~~~~~~~~~~~

``hg --debugger <command>`` will drop you at the debug prompt shortly before command execution. This will allow you to set breakpoints, singlestep code, inspect data structures, and run arbitrary bits of Python code. Help is available with '?'.

If you let Mercurial run (with 'cont'), the debugger will be reinvoked if an exception occurs. This is useful for diagnosing tracebacks in situ.

Using a better debugger
~~~~~~~~~~~~~~~~~~~~~~~

PuDB_ is a far more useful debugger than Python's native ``pdb``. To use PuDB:

1. pip install pudb

#. Add to ``~/.hgrc``:

   ::

       [ui]
       debugger = pudb

#. Invoke ``hg --debugger <command>`` as described above.

Debugging extensions
~~~~~~~~~~~~~~~~~~~~

Extensions haven't actually been loaded when the ``--debugger`` option lands you at the debugger prompt. You have to skip around a little to set a breakpoint in your extension:

::

   $ hg --debugger mycommand
   (Pdb) up
   (Pdb) b dispatch.runcommand
   (Pdb) c
   (Pdb) b extensions.find('myextension').mymodule.myfunction
   (Pdb) c

.. ############################################################################

.. _PuDB: https://documen.tician.de/pudb/

