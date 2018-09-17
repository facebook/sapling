Debugging Tests
===============

It's quite handy to be able to develop test scripts in parallel with the code (test-driven development). Hg's default ``tests/run-tests.py`` script described in :doc:`WritingTests` is a little slow for running a single test repeatedly while refining your test and code. And it runs the test in a temporary dir with an ever changing name. This is not useful for manually inspecting the state left by the test, or to run hg from within a debugger on an interim state.

Debugging test scripts
----------------------

``run-tests.py`` has a "debug" mode that disables the default "capture and diff" behavior and works with Python test scripts in addition to shell test scripts.  In debug mode, ``run-tests.py`` simply echos its child's stdout (and stderr).  Naturally, this makes it impossible for ``run-tests.py`` to tell if a test passed or failed: that's up to you to do by reading its output.  As above, this is particularly useful for running ``hg`` under the Python debugger.

Use the ``--debug`` option to activate debug mode:

::

   ./run-tests.py --debug test-something

(You can use debug mode on any number of test scripts, but in practice it's most useful on a single script.)

Debug mode has no effect on the temporary directory used to run tests; it will remain ``$TMPDIR/hgtests.XXXXXX``.  If you want to preserve and inspect the test environment, use ``--tmpdir`` to specify a different temporary directory.  Note that the meaning of ``--tmpdir`` changed in Mercurial 1.4: formerly, the test directory (``hgtests.XXXXXX``) was created inside the temp dir.  Now, ``--tmpdir`` *is* the test directory.  So if you are in the habit of running ``./run-tests.py --tmpdir /mnt/ramdisk``, you'll have to break that habit.  Instead, use ``TMPDIR=/mnt/ramdisk ./run-tests.py``.  ``run-tests.py`` will abort if the temp dir already exists to prevent accidents for people used to working this way.

Putting it all together, then, the way I most commonly use debug mode is this:

::

   rm -rf tmp && ./run-tests.py --local --debug --tmpdir tmp test-something

where:

* the ``rm -rf`` is necessary because ``run-tests.py`` requires that the tmp dir not exist, and will not blow it away for you (to prevent accidents)

* ``--local`` is optional, but speeds things up (saves the overhead of a throwaway build+install of Mercurial)

* ``--debug`` enables debug mode

* ``--tmpdir`` sets the test directory

If you need to sift through the wreckage after one run, it's in ``tmp/test-something``.  Also, keep in mind that ``run-tests.py`` creates a test-specific ``tmp/.hgrc``, so if you need to manually duplicate exactly what happened in the test script, you'll want to do something like this:

::

   cd tmp
   export HGRCPATH=$PWD/.hgrc
   cd test-something

And of course, most test scripts create one or more test repositories under that, so another level of ``cd`` is generally required.

Stepping into the python debugger
---------------------------------

Assuming a test is run as explained above, the python debugger can be activated by adding the ``--debugger`` option to ``hg`` in the test file. For instance in ``tests/test-basic.t``

::

     $ hg add a

could be changed for

::

     $ hg --debugger add a

and when the test is run it will break as follows:

::

   loic@bet:~/software/mercurial/mercurial/tests$ rm -rf tmp && python run-tests.py --local --debug --tmpdir tmp test-basic.t
   SALT1300291206.57 2 0
   SALT1300291206.57 3 0
   SALT1300291206.57 4 0
   > /home/loic/software/mercurial/mercurial/mercurial/dispatch.py(601)_dispatch()
   -> return runcommand(lui, repo, cmd, fullargs, ui, options, d,
   (Pdb)

An alternative to using the --debugger option is to force a breakpoint. For instance if 

::

           import pdb
           pdb.set_trace()

is added just before the ``runcommand`` call of ``mercurial/mercurial/dispatch.py`` it will break as follows, even if the test script is not modified. 

::

   > /home/loic/software/mercurial/mercurial/mercurial/dispatch.py(601)_dispatch()
   -> return runcommand(lui, repo, cmd, fullargs, ui, options, d,
   (Pdb)

You can also read more about the built-in :doc:`DebuggingFeatures` of hg.

Using Eclipse Pydev
-------------------

With the open-source `Eclipse Pydev Plugin`_, you can only launch ``hg`` directly from within `Eclipse <http://eclipse.org/>`__. So you need to stop the script before the command you want to debug (as described above) and setup a corresponding launch configuration in Eclipse.

Main module:

::

   ${workspace_loc:my-hg-project/hg}

Arguments (``...`` is the command in question):

::

   --cwd ~/dev/hg/tests/tmp/mytestrepo
   ...

Working dir (I had to switch from the default settings to this to make the debugger work):

::

   ${workspace_loc:my-hg-project}

Environment:

::

   HGRCPATH = ~/dev/hg/tests/tmp/hgrc

Using Eclipse Pydev extensions
------------------------------

With the commercially licensed `Pydev Extensions`_, you can attach to a running instance of ``hg``. This relies on a Pydev supplied module, whose path you need to set up. Amend the Python path in ``debug-test`` as follows:

::

   PYDEVDIR=/path/to/eclipse/plugins/org.python.pydev.debug_1.3.17/pysrc
   PYTHONPATH=$HGDIR:$PYDEVDIR:$PYTHONPATH

Then you can proceed as described in `Pydev's remote debugging instructions`_. Beware, though, that it did not work for me when placing ``pydevd.settrace()`` top-level in ``dispatch.py``. It did work within actual commands in ``commands.py`` and also deeper within the code. Don't despair if Pydev tries to locate ``demandimport.py`` and cannot find it. Simply set a breakpoint at your desired location and continue the debugger.

If your test script contains something like:

::

   hg stat
   ...
   hg stat

and you want to break on the second instance of ``hg stat`` only, you could use a flag file to signal this. I have the following in my ``debug-test`` script:

::

   rm /tmp/enable-hg-debugger
   DBG() {
       touch /tmp/enable-hg-debugger
   }

In the code I then do:

::

   if os.path.exists('/tmp/enable-hg-debugger'):
       import pydevd; pydevd.settrace()

and in the script I do:

::

   hg stat
   ...
   DBG; hg stat

.. ############################################################################

.. _Eclipse Pydev plugin: http://pydev.sourceforge.net/

.. _Pydev Extensions: http://www.fabioz.com/pydev/

.. _Pydev's remote debugging instructions: http://www.fabioz.com/pydev/manual_adv_remote_debugger.html

