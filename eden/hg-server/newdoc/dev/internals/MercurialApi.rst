The Mercurial API
=================

Rough introduction to Mercurial *internal* API.

Using this API is a strong indication that you're creating a "derived work" subject to the GPL.

Why you shouldn't use Mercurial's internal API
----------------------------------------------

Mercurial's internals are continually evolving to be simpler, more consistent, and more powerful, a process we hope will continue for the foreseeable future. Unfortunately, this process means we will regularly be changing interfaces in ways that break third-party code in various (mostly minor) ways.

For the vast majority of third party code, the best approach is to use *Mercurial's published, documented, and stable API*: the command line interface. Alternately, use the CommandServer_ or the libraries which are based on it to get a fast, stable, language-neutral interface.

There are NO guarantees that third-party code calling into Mercurial's internals won't break from release to release.

The high level interface
------------------------

It is possible to call Mercurial commands directly from within your code. Every Mercurial command corresponds to a function defined in the ``mercurial.commands`` module, with the calling signature

.. sourcecode:: python

       CMD(ui, repo, ...)

Here, ``ui`` and ``repo`` are the user interface and repository arguments passed into an extension function as standard (see WritingExtensions_ for more details). If you are not calling the Mercurial command functions from an extension, you will need to create suitable ui and repo objects yourself. The ui object can be instantiated from the ui class in mercurial.ui; the repo object can either be a localrepository, a httprepository, an sshrepository or a statichttprepository (each defined in their own modules), though it will most often be a localrepository.

The remainder of the parameters come in two groups:

* a sequence of positional parameters corresponding to the (non-option) command line arguments to the command

* a set of keyword parameters, corresponding to the command options - the key is the option name (long form) and the value is the value given to the option (or True/False if the option does not take an argument).

A reasonably complex example might be ``hg commit -m "A test" --addremove file1.py file2.py``. This would have an equivalent API form

.. sourcecode:: python

       from edenscm.mercurial import commands
       commands.commit(ui, repo, 'file1.py', 'file2.py', message="A test", addremove=True)

In practice, some of the options for the commit command are required in a call, and must be included as keyword parameters - adding ``date=None, user=None, logfile=None`` would be sufficient in this case. This detail can be ignored for now.

Commands which fail will raise a mercurial.error.Abort exception, with a message describing the problem:

.. sourcecode:: python

       from edenscm.mercurial import error
       raise error.Abort("The repository is not local")

Generally, however, you should **not** use this interface, as it mixes user interface and functionality. If you want to write robust code, you should read the source of the command function, and extract the relevant details. For most commands, this is not as hard as it seems - there is often a "core" function (usually in the ``cmdutil`` or ``hg`` module) which performs the important work of the command.

Setting up repository and UI objects
------------------------------------

In order to get started, you'll often need a UI and a repository object. The UI object keeps access to input and output objects and all the relevant config bits (machine-global, user-global, repo-wide, and specific for this invocation), the repository represents, well, the repository. A repository object can be any of a number of objects (as enumerated below), but these two lines make it easy to create an appropriate repository object:

.. sourcecode:: python

       from edenscm.mercurial import ui, hg
       repo = hg.repository(ui.ui(), '.')

Here, the '.' is the path to the repository, which could also be something starting with http or ssh, for example. You'll often need these objects to get any work done through the Mercurial API, for example by using the commands as detailed above.

mercurial.ui instances have two flavors - global and repo.  When you instantiate a new ui instance, it automatically reads all of the site-wide and user config files.  When you pass a ui instance to hg.repository(), the repo copies it, then reads (adds) its repository configuration.  Global ui instances are interchangeable, but once it has included repository setup you don't want to use it again for another repository, else you get bleed-through.

Communicating with the user
---------------------------

Most extensions will need to perform some interaction with the user. This is the purpose of the ``ui`` parameter to an extension function. The ``ui`` parameter is an object with a number of useful methods for interacting with the user.

Writing output:

* ``ui.write(*msg)`` - write a message to the standard output (the message arguments are concatenated). This should only be used if you really want to give the user no way of suppressing the output. ``ui.status`` (below) is usually better.

* ``ui.status(*msg)`` - write a message at status level (shown unless --quiet is specified)

* ``ui.note(*msg)`` - write a message at note level (shown if --verbose is specified)

* ``ui.debug(*msg)`` - write a message at debug level (shown if --debug is specified)

* ``ui.warn(*msg)`` - write a warning message to the error stream

* ``ui.flush()`` - flush the output and error streams

Accepting input:

* ``ui.prompt(msg, default="y")`` - prompt the user with MSG and read the response. If we are not in an interactive context, just return DEFAULT.

* ``ui.promptchoice(prompt, default=0)`` - Prompt user with a message, read response, and ensure it matches one of the provided choices. The prompt is formatted as follows: 

    "would you like fries with that (Yn)? $$ &Yes $$ &No"

  The index of the choice is returned. Responses are case insensitive. If ui is not interactive, the default is returned.

* ``ui.edit(text, user)`` - open an editor on a file containing TEXT. Return the edited text, with lines starting ``HG:`` removed. While the edit is in progress, the HGUSER environment variable is set to USER.

Useful values:

* ``ui.geteditor()`` - the user's preferred editor

* ``ui.username()`` - the default username to be used in commits

* ``ui.shortuser(user)`` - a short form of user name USER

* ``ui.expandpath(loc, default=None)`` - the location of repository LOC (which may be relative to the CWD, or from the [paths] configuration section. If no other value can be found, DEFAULT is returned.

Collecting output
~~~~~~~~~~~~~~~~~

Output from a ``ui`` object is usually to the standard output, ``sys.stdout``. However, it is possible to "divert" all output and collect it for processing by your code. This involves the ``ui.pushbuffer()`` and ``ui.popbuffer()`` functions. At the start of the code whose output you want to collect, call ``ui.pushbuffer()``. Then, when you have finished the code whose output you wish to collect, call ``ui.popbuffer()``. The ``popbuffer()`` call returns all collected output as a string, for you to process as you wish (and potentially pass to ``ui.write()``) in some form, if you just want to edit the output and then send it on.

Here is a sample code snippet adapted from http://selenic.com/pipermail/mercurial/2010-February/030231.html:

.. sourcecode:: python

   from edenscm.mercurial import ui, hg, commands
   u = ui.ui()
   repo = hg.repository(u, "/path/to/repo")
   u.pushbuffer()
   # command / function to call, for example:
   commands.log(u, repo)
   output = u.popbuffer()
   assert type(output) == str

Reading configuration files
~~~~~~~~~~~~~~~~~~~~~~~~~~~

All relevant configuration values should be represented in the UI object -- that is, global configuration (``/etc/mercurial/hgrc``), user configuration (``~/.hgrc``) and repository configuration (``.hg/hgrc``). You can easily read from these using the following methods on the ui object:

* ``ui.config(section, name, default=None, untrusted=False)`` - gets a configuration value, or a default value if none is specified

* ``ui.configbool(section, name, default=False, untrusted=False``) - convert a config value to boolean (Mercurial accepts several different spellings, like True, false and 0)

* ``ui.configlist(section, name, default=None, untrusted=False)`` - try to make a list from the requested config value. The elements are separated by comma or whitespace.

* ``ui.configitems(section, untrusted=False)`` - return all configuration values in the given section

Repositories
------------

There are a number of different repository types, each defined with its own class name, in its own module. All repository types are subclasses of ``mercurial.repo.repository``.

------------  ------------------------  -------------------------
*Protocol*    *Module*                  *Class Name*
------------  ------------------------  -------------------------
local         mercurial.localrepo       ``localrepository``
http          mercurial.httprepo        ``httprepository``
static-http   mercurial.statichttprepo  ``statichttprepository``
ssh           mercurial.sshrepo         ``sshrepository``
bundle        mercurial.bundlerepo      ``bundlerepository``
------------  ------------------------  -------------------------

Repository objects should be created using ``module.instance(ui, path, create)`` where ``path`` is an appropriate path/URL to the repository, and ``create`` should be ``True`` if a new repository is to be created. You can also use the helper method hg.repository(), which selects the appropriate repository class based on the path or URL passed.

Repositories have many methods and attributes, but not all repository types support all of the various options.

Some key methods of (local) repositories:

* ``repo[changeid]`` - a change context for the changeset ``changeid``. changid can be a descriptor like changeset hash, revision number, 'tip', '.', branch names, tags or anything that can be resolved to a changeset hash.

* ``repo[None]`` - a change context for the working directory

* ``repo.changelog`` - the repository changelog

* ``repo.root`` - the path of the repository root

* ``repo.status()`` - returns a tuple of files modified, added, removed, deleted, unknown(?), ignored and clean in the current working directory

Change contexts
---------------

A change context is an object which provides convenient access to various data related to a particular changeset. Change contexts can be converted to a string (for printing, etc - the string representation is the short ID), tested for truth value (false is the null revision), compared for equality, and used as keys in a dictionary. They act as containers for filenames - all of the following work:

* ``filename in changectx`` - tests if the file is in the changeset

* ``changectx[filename]`` - returns the file context

* ``for filename in changectx`` - loops over all files in the changeset (in sorted order)

Some informational methods on change context objects:

* ``ctx.rev()`` - the revision number

* ``ctx.node()`` - the revision ID (as 20 bytes in an array)

* ``ctx.hex()`` - the revision ID (as 40 characters suitable for printing)

* ``ctx.user()`` - the user who created the changeset

* ``ctx.date()`` - the date of the changeset

* ``ctx.files()`` - the files changed in the changeset

* ``ctx.description()`` - the changeset log message

* ``ctx.branch()`` - the branch of the changeset

* ``ctx.tags()`` - a list of the tags applied to the changeset

* ``ctx.parents()`` - a list of the change context objects for the changeset's parents

* ``ctx.children()`` - a list of the change context objects for the changeset's children

* ``ctx.filectx(path)`` - get a filecontext, the same as ``ctx[path]``

* ``ctx.ancestor(c2)`` - the common ancestor change context of ``ctx`` and ``c2``

File contexts
-------------

A file context is an object which provides convenient access to various data related to a particular file revision. File contexts can be converted to a string (for printing, etc - the string representation is the "path@shortID"), tested for truth value (False is "nonexistent"), compared for equality, and used as keys in a dictionary.

Some informational methods on file context objects:

* ``fctx.filectx(id)`` - the file context for another revision of the file

* ``fctx.filerev()`` - the revision at which this file was last changed

* ``fctx.filenode()`` - the file ID

* ``fctx.fileflags()`` - the file flags

* ``fctx.isexec()`` - is the file executable

* ``fctx.islink()`` - is the file a symbolic link

* ``fctx.filelog()`` - the file log for the file revision (file logs are not documented here - see the source)

* ``fctx.rev()`` - the revision from which this file context was extracted

* ``fctx.changectx()`` - the change context associated with this file revision

* ``fctx.node``, ``fctx.user``, ``fctx.date``, ``fctx.files``, ``fctx.description``, ``fctx.branch``, ``fctx.manifest`` - the same as the equivalent change context methods, applied to the change context associated with the file revision.

* ``fctx.data()`` - the file data

* ``fctx.path()`` - the file path

* ``fctx.size()`` - the file size

* ``fctx.isbinary()`` - the file is binary

* ``fctx.cmp(fctx)`` - does the file contents differ from another file contents?

* ``fctx.annotate(follow=False, linenumber=None)`` - list of tuples of ``(ctx, line)`` for each line in the file, where ctx is the file context of the node where that line was last changed. (The follow and linenumber parameters are not documented here - see the source for details).

Revlogs
-------

Revlogs_ are the storage backend for Mercurial. They are not fully documented here, as it is unlikely that extension code will require detailed access to revlogs. However, a couple of key methods which may be generally useful are:

* ``len(log)`` - the number of revisions in the changelog

* ``log.tip()`` - the ID of the tip revision

Unicode and user data
---------------------

Don't pass Unicode strings to Mercurial APIs!

All Mercurial internals pass byte strings exclusively. The vast majority of these are encoded and manipulated in the "local" encoding (as set in '``encoding.encoding``'). Code that passes Unicode objects will almost certainly break as soon it's used with non-ASCII data. The '``encoding.fromlocal()``' and '``tolocal()``' functions will handle transcoding from the "local" encoding to UTF-8 byte strings.

Don't transcode non-metadata!

Mercurial aims to preserve user's project data (filenames and file contents) byte-for-byte, so converting such data to Unicode and back is potentially destructive. Only metadata such as usernames and changeset descriptions are considered to be in a known encoding (stored as UTF-8 internally). See `Encoding Strategy`_.

.. ############################################################################

.. _CommandServer: CommandServer

.. _WritingExtensions: ../process/WritingExtensions

.. _Revlogs: RevlogNG

.. _Encoding Strategy: EncodingStrategy

