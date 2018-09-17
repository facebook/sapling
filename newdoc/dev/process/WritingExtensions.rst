Writing Mercurial Extensions
============================

Mercurial features an extension mechanism for adding new commands.

Extensions allow the creation of new features and using them directly from the main hg command line as if they were built-in commands. The extensions have full access to the *internal* MercurialApi_.

Use of Mercurial's internal API very likely makes your code subject to Mercurial's license.

There are NO guarantees that third-party code calling into Mercurial's internals won't break from release to release.

.. contents::

File Layout
-----------

Extensions are usually written as simple python modules. Larger ones are better split into multiple modules of a single package. The package root module gives its name to the extension and implements the ``cmdtable`` and optional callbacks described below.

Command table
-------------

To write your own extension, your python module can provide an optional dict named ``cmdtable`` with entries describing each command. A command should be registered to the ``cmdtable`` by ``@command`` decorator.

Example using ``@command`` decorator:

.. sourcecode:: python

   from mercurial import cmdutil
   from mercurial.i18n import _
   cmdtable = {}
   command = cmdutil.command(cmdtable)
   @command('print-parents',
       [('s', 'short', None, _('print short form')),
        ('l', 'long', None, _('print long form'))],
       _('[options] node'))
   def printparents(ui, repo, node, **opts):
       ...

The cmdtable dictionary
~~~~~~~~~~~~~~~~~~~~~~~

The ``cmdtable`` dictionary uses as key the new command names, and, as value, a tuple containing:

1. the function to be called when the command is used.

#. a list of options the command can take.

#. a command line synopsis for the command (the function docstring is used for the full help).

List of options
~~~~~~~~~~~~~~~

All the command flag options are documented in the ``mercurial/fancyopts.py`` sources.

The options list is a list of tuples containing:

1. the short option letter, or ``''`` if no short option is available (for example, ``o`` for a ``-o`` option).

#. the long option name (for example, ``option`` for a ``--option`` option).

#. a default value for the option.

#. a help string for the option (it's possible to omit the "hg newcommand" part and only the options and parameter substring is needed).

Command function signatures
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Functions that implement new commands always receive a ``ui`` and usually a ``repo`` parameter. Please see the MercurialApi_ for information on how to use these. The rest of parameters are taken from the command line items that don't start with a dash and are passed in the same order they were written.  If no default value is given in the parameter list they are required.

If there is no repo to be associated with the command and consequently no ``repo`` passed, then ``norepo=True`` should be passed to the ``@command`` decorator.

.. sourcecode:: python

   @command('mycommand', [], norepo=True)
   def mycommand(ui, **opts):
       ...

Command function docstrings
---------------------------

The docstring of your function is used as the main help text, shown by ``hg help mycommand``. The docstring should be formatted using a simple subset of reStructuredText_ markup. The supported constructs include:

Paragraphs:

::

   This is a paragraph.

   Paragraphs are separated
   by blank lines.

A verbatim block is introduced with a double colon followed by an indented block. The double colon is turned into a single colon on display:

::

   Some text::

     verbatim
       text
        !!

We have field lists:

::

   :key1: value1
   :key2: value2

Bullet lists:

::

   - foo
   - bar

Enumerated lists:

::

   1. foo
   2. bar

Inline markup: ``*bold*``, ````monospace````. Mark Mercurial commands as ``:hg:`command``` to make a nice link to the corresponding documentation. We'll expand the support if new constructs can be parsed without too much trouble.

Communicating with the user
---------------------------

Besides the ``ui`` methods listed in MercurialApi_, like ``ui.write(*msg)`` or ``ui.prompt(msg, default="y")``, an extension can add help text for each of its commands and the extension itself.

The module docstring will be used as help string when ``hg help extensionname`` is used and, similarly, the help string for a command and the docstring belonging to the function that's wrapped by the command will be shown when ``hg help command`` is invoked.

Setup Callbacks
---------------

Extensions are loaded in phases. All extensions are processed in a given phase before the next phase begins. In the first phase, all extension modules are loaded and registered with Mercurial. This means that you can find all enabled extensions with ``extensions.find`` in the following phases.

ui setup
~~~~~~~~

Extensions can implement an optional callback named ``uisetup``. ``uisetup`` is called when the extension is first loaded and receives a ui object:

.. sourcecode:: python

   def uisetup(ui):
       # ...

Extension setup
~~~~~~~~~~~~~~~

Extensions can implement an optional callback named ``extsetup``. It is called after all the extension are loaded, and can be useful in case one extension optionally depends on another extension. Signature:

.. sourcecode:: python

   def extsetup(ui):
       # ...

Command table setup
~~~~~~~~~~~~~~~~~~~

After ``extsetup``, the ``cmdtable`` is copied into the global command table in Mercurial.

Repository setup
~~~~~~~~~~~~~~~~

Extensions can implement an optional callback named ``reposetup``. It is called after the main Mercurial repository initialization, and can be used to setup any local state the extension might need.

As other command functions it receives an ``ui`` object and a ``repo`` object (no additional parameters for this, though):

.. sourcecode:: python

   def reposetup(ui, repo):
       #do initialization here.

It is important to take into account that the ``ui`` object that is received by the ``reposetup`` function is not the same as the one received by the ``uisetup`` and ``extsetup`` functions. This is particularly important when setting up hooks as described in the following section, since not all hooks use the same ``ui`` object and hence different hooks must be configured in different setup functions.

Wrapping methods on the ui and repo classes
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Because extensions can be loaded *per repository*, you should avoid using ``extensions.wrapfunction()`` on methods of the ``ui`` and ``repo`` objects. Instead, create a subclass of the specific class of the instance passed into the ``*setup()`` hook; e.g. use ``ui.__class__`` as the base class, then reassign your new class to ``ui.__class__`` again. Mercurial will then use your updated ``ui`` or ``repo`` instance only for repositories where your extension is enabled (or copies thereof, reusing your new class).

For example:

.. sourcecode:: python

   def uisetup(ui):
       class echologui(ui.__class__):
           def log(self, service, *msg, **opts):
               if msg:
                   self.write('%s: %s\n' % (service, msg[0] % msg[1:]))
               super(echologui, self).log(service, *msg, **opts)
      
       ui.__class__ = echologui

     

Configuring Hooks
-----------------

Some extensions must use hooks to do their work. These required hooks can be configured manually by the user by modifying the ``[hook]`` section of their hgrc, but they can also be configured automatically by calling the ``ui.setconfig('hooks', ...)`` function in one of the setup functions described above.

The main difference between manually modifying the hooks section in the hgrc and using ``ui.setconfig()`` is that when using ``ui.setconfig()`` you have access to the actual hook function object, which you can pass directly to ``ui.setconfig()``, while when you use the hooks section of the hgrc file you must refer to the hook function by using the "``python:modulename.functioname``" idiom (e.g. "``python:hgext.notify.hook``").

For example:

.. sourcecode:: python

   # Define hooks -- note that the actual function name it irrelevant.
   def preupdatehook(ui, repo, **kwargs):
       ui.write("Pre-update hook triggered\n")

   def updatehook(ui, repo, **kwargs):
       ui.write("Update hook triggered\n")

   def uisetup(ui):
       # When pre-<cmd> and post-<cmd> hooks are configured by means of
       # the ui.setconfig() function, you must use the ui object passed
       # to uisetup or extsetup.
       ui.setconfig("hooks", "pre-update.myextension", preupdatehook)

   def reposetup(ui, repo):
       # Repository-specific hooks can be configured here. These include
       # the update hook.
       ui.setconfig("hooks", "update.myextension", updatehook)

Note how different hooks may need to be configured in different setup functions. In the example you can see that the ``update`` hook must be configured in the ``reposetup`` function, while the ``pre-update`` hook must be configured on the ``uisetup`` or the ``extsetup`` functions.

Marking compatible versions
---------------------------

Every extension should use the ``testedwith`` variable to specify Mercurial releases it's known to be compatible with. This helps us and users diagnose where problems are coming from. 

.. sourcecode:: python

   testedwith = '2.0 2.0.1 2.1 2.1.1 2.1.2'

Do not use the ``internal`` marker in third-party extensions; we will immediately drop all bug reports mentioning your extension if we catch you doing this.

Similarly, an extension can use the ``buglink`` variable to specify how users should report issues with the extension.  This link will be included in the error message if the extension produces errors.

.. sourcecode:: python

   buglink = 'https://bitbucket.org/USER/REPO/issues'

Example extension
-----------------

.. sourcecode:: python

   """printparents
   Prints the parents of a given revision.
   """
   from mercurial import cmdutil, error
   from mercurial.i18n import _
   cmdtable = {}
   command = cmdutil.command(cmdtable)
   testedwith = '2.2 2.3'
   # Every command must take ui and and repo as arguments.
   # opts is a dict where you can find other command line flags.
   #
   # Other parameters are taken in order from items on the command line that
   # don't start with a dash. If no default value is given in the parameter list,
   # they are required.
   #
   # For experimenting with Mercurial in the python interpreter:
   # Getting the repository of the current dir:
   #    >>> from mercurial import hg, ui
   #    >>> repo = hg.repository(ui.ui(), path = ".")
   @command('print-parents',
       [('s', 'short', None, _('print short form')),
        ('l', 'long', None, _('print long form'))],
       _('[options] node'))
   def printparents(ui, repo, node, **opts):
       # The doc string below will show up in hg help.
       """Print parent information."""
       # repo can be indexed based on tags, an sha1, or a revision number.
       ctx = repo[node]
       parents = ctx.parents()
       try:
           if opts['short']:
               # The string representation of a context returns a smaller portion
               # of the sha1.
               ui.write(_('short %s %s\n') % (parents[0], parents[1]))
           elif opts['long']:
               # The hex representation of a context returns the full sha1.
               ui.write(_('long %s %s\n') % (parents[0].hex(), parents[1].hex()))
           else:
               ui.write(_('default %s %s\n') % (parents[0], parents[1]))
       except IndexError:
           # Raise an Abort exception if the node has only one parent.
           raise error.Abort(_('revision %s has only one parent') % node)

If ``cmdtable`` or ``reposetup`` is not present, your extension will still work.  This means that an extension can work "silently", without making new functionality directly visible through the command line interface.

Testing the example extension
-----------------------------

This is a test for example extension above:

::

   Test printparents extension.

   Activate the printparents extension:
     $ echo "[extensions]" >> $HGRCPATH
     $ echo "printparents=" >> $HGRCPATH

   Create a new repo:
     $ hg init r
     $ cd r

   Add two new files and commit them separately:
     $ echo c1 > f1
     $ hg commit -Am 0
     adding f1
     $ echo c2 > f2
     $ hg commit -Am 1
     adding f2

   Update to revision 0. Add and commit a third file creating a new head:
     $ hg up 0
     0 files updated, 0 files merged, 1 files removed, 0 files unresolved
     $ echo c3 > f3
     $ hg commit -Am 2
     adding f3
     created new head

   Merge the two heads and commit:
     $ hg merge
     1 files updated, 0 files merged, 0 files removed, 0 files unresolved
     (branch merge, don't forget to commit)
     $ hg commit -m 3

   Test printparents with the (merged) tip:
     $ hg print-parents tip
     default 33960aadc16f c3adabd1a5f4

   Testing printparents with revision 2 will fail (because there is only one parent):
     $ hg print-parents 2
     abort: revision 2 has only one parent
     [255]

Learn more about testing Mercurial: WritingTests_

Wrap up: what belongs where?
----------------------------

You will find here a list of most common tasks, based on setups from the extensions included in Mercurial core.

uisetup
~~~~~~~

* Changes to ``ui.__class__`` . The ``ui`` object that will be used to run the command has not yet been created. Changes made here will affect ``ui`` objects created after this, and in particular the ``ui`` that will be passed to ``runcommand``

* Command wraps (``extensions.wrapcommand``)

* Changes that need to be visible by other extensions: because initialization occurs in phases (all extensions run ``uisetup``, then all run ``extsetup``), a change made here will be visible by other extensions during ``extsetup``

* Monkeypatches or function wraps (``extensions.wrapfunction``) of ``dispatch`` module members

* Setup of pre-* and post-* hooks

* ``pushkey`` setup

extsetup
~~~~~~~~

* Changes depending on the status of other extensions. (``if extensions.find('mq')``)

* Add a global option to all commands

* Extend revsets

reposetup
~~~~~~~~~

* All hooks but pre-* and post-*

* Modify configuration variables

* Changes to ``repo.__class__``, ``repo.dirstate.__class__``

.. ############################################################################

.. _MercurialApi: ../internals/MercurialApi

.. _reStructuredText: http://docutils.sourceforge.net/docs/user/rst/quickstart.html

.. _WritingTests: WritingTests

