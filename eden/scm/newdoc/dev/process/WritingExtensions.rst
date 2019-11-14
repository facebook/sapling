Writing Extensions
==================

Extensions are Python modules that live in ``edenscm/hgext/``.

These modules should be side-effect free when imported.

.. note::

   In the future, extensions might be implemented in other languages using
   different sets of APIs. This document is only about Python extensions.

Registering new logic
---------------------

Use the ``registrar`` module to register commands, revsets, template keywords,
config items, and namespaces. They will be defined as module-level variables
and will not change the global state living in other modules.

New commands
~~~~~~~~~~~~

Here is an example of registering a simple ``hello`` command:

.. sourcecode:: python

    from edenscm.mercurial import registrar
    from edenscm.mercurial.i18n import _

    cmdtable = {}
    command = registrar.command(cmdtable)

    @command('hello', norepo=True)
    def hello(ui):
        """print ``Hello!``"""
        ui.write(_('Hello!\n'))

The docstring is used as help text. Match the style with existing commands. For
example, the first line should be a short, non-sentence (no dot, no capital
letter) description of the command. It's also in reStructuredText, not
Markdown. Check the docstring of existing core commands for its syntax.

Search ``@command`` in existing code to find more examples, like how to add
command line flags, etc.

On command-line flag handling, one common mistake in command-line flag handling
is that ``--rev`` often accepts a list of revsets. That is, the user can pass
things like ``-r foo+bar -r baz``. Treating it as a single revision, or a list
of simple names (ex. bookmarks or commit hashes) are incorrect.

New revsets
~~~~~~~~~~~

Here is an example of registering a ``draftbranch`` revset:

.. sourcecode:: python

    from edenscm.mercurial import registrar, revset, smartset

    revsetpredicate = registrar.revsetpredicate()

    @revsetpredicate("draftbranch([set])")
    def draftbranch(repo, subset, x=None):
        """The set of all commits in feature branches containing the current
        or selected commits.
        """
        if x is None:
            revs = [p.rev() for p in repo[None].parents()]
        else:
            revs = revset.getset(revset.getset(repo, smartset.fullreposet(repo), x))

        return subset & smartset.baseset(repo.revs("roots(draft() & ::%ld)::", revs))

The docstring is used as help text in ``hg help revset`` command.


Patching existing logic
-----------------------

For things that cannot be done using the ``registrar`` framework, patching
existing logic is often the choice. Importing an extension should be
side-effect free, so the patching logic needs to be defined at entry points
like ``uisetup`` or ``reposetup``.

``uisetup`` is called when the extension is first loaded and receives a ui
object. ``reposetup`` is called after the main Mercurial repository
initialization, and can be used to alter the repo state.

Patching Python methods
~~~~~~~~~~~~~~~~~~~~~~~

The helper function ``extensions.wrapfunction`` provides a way to patch
existing methods.

Here is an example that patches ``os.unlink`` to show a prompt before
unlinking:

.. sourcecode:: python

    from edenscm.mercurial.i18n import _
    from edenscm.mercurial import error
    import os

    def uisetup(ui):
        def promptunlink(orig, path):
            if ui.prompt(_('delete %r [yn]?') % path) != 'y':
                raise error.Abort(_('refuse to delete %r') % path)
            return orig(path)

        extensions.wrapfunction(os, 'unlink', promptunlink)

Patching methods on the ``ui`` or ``repo`` object
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

While it's possible to patch methods on ``ui.ui``, or
``localrepo.localrepository`` object using the above method, other extensions
might change the class of those objects. To work better with other extensions,
just replace ``__class__`` is the better way.

Here is an example that patches ``repo.lock`` method to forbid writes (because
writes need to take the lock):

.. sourcecode:: python

    from edenscm.mercurial import error
    from edenscm.mercurial.i18n import _

    def reposetup(ui, repo):
        class readonlyrepo(repo.__class__):
            def lock(self, *args, **kwargs):
                raise error.Abort(_('write is forbidden!'))

        repo.__class__ = readonlyrepo

