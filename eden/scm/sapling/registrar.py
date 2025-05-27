# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# registrar.py - utilities to register function for specific purpose
#
#  Copyright FUJIWARA Katsunori <foozy@lares.dti.ne.jp> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from . import error, identity, util


class _funcregistrarbase:
    """Base of decorator to register a function for specific purpose

    This decorator stores decorated functions into own dict 'table'.

    The least derived class can be defined by overriding 'formatdoc',
    for example::

        class keyword(_funcregistrarbase):
            _docformat = ":%s: %s"

    This should be used as below:

        keyword = registrar.keyword()

        @keyword('bar')
        def barfunc(*args, **kwargs):
            '''Explanation of bar keyword ....
            '''
            pass

    In this case:

    - 'barfunc' is stored as 'bar' in '_table' of an instance 'keyword' above
    - 'barfunc.__doc__' becomes ":bar: Explanation of bar keyword"
    """

    def __init__(self, table=None):
        if table is None:
            self._table = util.sortdict()
        else:
            self._table = table

    def __call__(self, decl, *args, **kwargs):
        return lambda func: self._doregister(func, decl, *args, **kwargs)

    def _doregister(self, func, decl, *args, **kwargs):
        name = self._getname(decl)

        if name in self._table:
            msg = 'duplicate registration for name: "%s"' % name
            raise error.ProgrammingError(msg)

        if func.__doc__ and not hasattr(func, "_origdoc"):
            doc = func.__doc__.strip()
            func._origdoc = doc
            func.__doc__ = self._formatdoc(decl, doc)

        self._table[name] = func
        self._extrasetup(name, func, *args, **kwargs)

        return func

    def _parsefuncdecl(self, decl):
        """Parse function declaration and return the name of function in it"""
        i = decl.find("(")
        if i >= 0:
            return decl[:i]
        else:
            return decl

    def _getname(self, decl):
        """Return the name of the registered function from decl

        Derived class should override this, if it allows more
        descriptive 'decl' string than just a name.
        """
        return decl

    _docformat = ""

    def _formatdoc(self, decl, doc):
        """Return formatted document of the registered function for help

        'doc' is '__doc__.strip()' of the registered function.
        """
        return self._docformat % (decl, doc)

    def _extrasetup(self, name, func):
        """Execute extra setup for registered function, if needed"""


class command(_funcregistrarbase):
    """Decorator to register a command function to table

    This class receives a command table as its argument. The table should
    be a dict.

    The created object can be used as a decorator for adding commands to
    that command table. This accepts multiple arguments to define a command.

    The first argument is the command name (as bytes).

    The `options` keyword argument is an iterable of tuples defining command
    arguments. See existing commands for the format of each tuple.

    The `synopsis` argument defines a short, one line summary of how to use the
    command. This shows up in the help output.

    There are three arguments that control what repository (if any) is found
    and passed to the decorated function: `norepo`, `optionalrepo`, and
    `inferrepo`.

    The `norepo` argument defines whether the command does not require a
    local repository. Most commands operate against a repository, thus the
    default is False. When True, no repository will be passed.

    The `optionalrepo` argument defines whether the command optionally requires
    a local repository. If no repository can be found, None will be passed
    to the decorated function.

    The `inferrepo` argument defines whether to try to find a repository from
    the command line arguments. If True, arguments will be examined for
    potential repository locations. See ``findrepo()``. If a repository is
    found, it will be used and passed to the decorated function.

    The `cmdtemplate` argument defines whether to enable command-level template
    support. Once turned on, the command's output can be redefined by `-T`
    template language entirely. If `-T` is provided, traditional `ui.write`
    outputs are suppressed. The command entry point would get a `templ`
    argument after `repo` for manipulating the data source to render the
    template.

    There are three constants in the class which tells what type of the command
    that is. That information will be helpful at various places. It will be also
    be used to decide what level of access the command has on hidden commits.
    The constants are:

    `unrecoverablewrite` is for those write commands which can't be recovered
    like push.
    `recoverablewrite` is for write commands which can be recovered like commit.
    `readonly` is for commands which are read only.

    The `subonly` argument defines whether the command requires a subcommand to
    be called.  Any command may have subcommands, however if `subonly` is true
    then there will be an error produced (and help text shown) if the user calls
    the command without a subcommand.

    The `legacyaliases` argument defines legacy aliases for historical Mercurial
    users. They will not be available when run as Sapling.

    The signature of the decorated function looks like this:
        def cmd(ui[, repo] [, <args>] [, <options>])

      `repo` is required if `norepo` is False.
      `<args>` are positional args (or `*args`) arguments, of non-option
      arguments from the command line.
      `<options>` are keyword arguments (or `**options`) of option arguments
      from the command line.

    See the WritingExtensions and MercurialApi documentation for more exhaustive
    descriptions and examples.
    """

    unrecoverablewrite = "unrecoverable"
    recoverablewrite = "recoverable"
    readonly = "readonly"

    possiblecmdtypes = {unrecoverablewrite, recoverablewrite, readonly}

    showlegacynames = "hg" in identity.default().cliname()

    def _doregister(
        self,
        func,
        name,
        options=(),
        synopsis=None,
        norepo=False,
        optionalrepo=False,
        inferrepo=False,
        cmdtemplate=False,
        cmdtype=unrecoverablewrite,
        subonly=False,
        legacyaliases=[],
        legacyname=None,
    ):
        def subcommand(table=None, categories=None):
            c = command(table)
            func.subcommands = c._table
            func.subcommandcategories.extend(categories or [])
            return c

        if cmdtype not in self.possiblecmdtypes:
            raise error.ProgrammingError(
                "unknown cmdtype value '%s' for '%s' command" % (cmdtype, name)
            )

        nameparts = name.split("|")
        primaryname = nameparts[0]

        if self.showlegacynames:
            if legacyname:
                nameparts = [legacyname, *nameparts]

            if legacyaliases:
                nameparts += legacyaliases

        name = "|".join(nameparts)

        if primaryname in self._table:
            # This is for compat w/ Rust. Rust is expected to not
            # include aliases for commands that exist in Rust and
            # Python because Python aliases take precedence.
            self._table[name] = self._table.pop(primaryname)

        func.norepo = norepo
        func.optionalrepo = optionalrepo
        func.inferrepo = inferrepo
        func.cmdtemplate = cmdtemplate
        func.cmdtype = cmdtype
        func.subcommand = subcommand
        func.subcommands = {}
        func.subcommandcategories = []
        func.subonly = subonly
        func.namesforhooks = list(filter(None, [primaryname, legacyname]))
        func.legacyname = legacyname

        if name in self._table:
            # If the command already was in the table it is because it was an existing Rust command.
            # We should keep and show the documentation for the Rust command. Since some Rust commands still
            # fall back into the Python command in some scenarios, we cannot entirely keep the Rust function
            if util.istest() and (util.getdoc(func) or synopsis):
                msg = 'duplicate help message for name: "%s"' % name
                raise error.ProgrammingError(msg)
            prevfunc, *helpargs = self._table[name]
            func.__rusthelp__ = util.getdoc(prevfunc), *helpargs

        if synopsis:
            self._table[name] = func, list(options), synopsis
        else:
            self._table[name] = func, list(options)

        return func


class namespacepredicate(_funcregistrarbase):
    """Decorator to register namespace predicate

    Usage::

        namespacepredicate = registrar.namespacepredicate()

        @namespacepredicate('mynamespace', priority=50)
        def getmynamespace(repo):
            return namespaces.namespace(...)

    Argument 'priority' will be used to decide the order. Smaller priority will
    be inserted first. If namespaces have a same priority, their names will be
    used, and inserted by the alphabet order.

    The function can read configurations from 'repo' and decide to not
    add the namespace by returning 'None'.

    In most cases, the priority should be higher than the builtinnamespaces.
    See namespaces.py for priorities of builtin namespaces.
    """

    _docformat = "``%s``\n    %s"

    def _extrasetup(self, name, func, priority):
        if priority is None:
            raise error.ProgrammingError("namespace priority must be specified")

        func._priority = priority


class autopullpredicate(_funcregistrarbase):
    """Decorator to register autopull predicate

    Usage::

        autopullpredicate = registrar.autopullpredicate()

        @autopullpredicate('myname', priority=50)
        def myname(repo, name):
            return autopull.pullattempt(...)  # if autopull is needed

    Argument 'priority' will be used to decide the order. Smaller priority will
    be processed first.

    If 'rewritepullrev' is True, the autopull function is also used to rewrite
    arguments of 'pull -r'. This is useful to translate 'pull -r D123' to
    'pull -r COMMIT_HASH'. The function must take an extra boolean
    'rewritepullrev' argument, which will be set to True during 'pull -r'
    resolution.

    The function should return True if it pulled something. Otherwise it should
    return None or False.
    """

    _docformat = "``%s``\n    %s"

    def _extrasetup(self, name, func, priority, rewritepullrev=False):
        if priority is None:
            raise error.ProgrammingError("autopull priority must be specified")
        func._rewritepullrev = rewritepullrev
        func._priority = priority


class revsetpredicate(_funcregistrarbase):
    """Decorator to register revset predicate

    Usage::

        revsetpredicate = registrar.revsetpredicate()

        @revsetpredicate('mypredicate(arg1, arg2[, arg3])')
        def mypredicatefunc(repo, subset, x):
            '''Explanation of this revset predicate ....
            '''
            pass

    The first string argument is used also in online help.

    Optional argument 'safe' indicates whether a predicate is safe for
    DoS attack (False by default).

    Optional argument 'takeorder' indicates whether a predicate function
    takes ordering policy as the last argument.

    Optional argument 'weight' indicates the estimated run-time cost, useful
    for static optimization, default is 1. Higher weight means more expensive.
    Usually, revsets that are fast and return only one revision has a weight of
    0.5 (ex. a symbol); revsets with O(changelog) complexity and read only the
    changelog have weight 10 (ex. author); revsets reading manifest deltas have
    weight 30 (ex. adds); revset reading manifest contents have weight 100
    (ex. contains). Note: those values are flexible. If the revset has a
    same big-O time complexity as 'contains', but with a smaller constant, it
    might have a weight of 90.

    'revsetpredicate' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'revsetpredicate' is used for
    decorating in extension.

    Otherwise, explicit 'revset.loadpredicate()' is needed.
    """

    _getname = _funcregistrarbase._parsefuncdecl
    _docformat = "``%s``\n    %s"

    def _extrasetup(self, name, func, safe=False, takeorder=False, weight=1):
        func._safe = safe
        func._takeorder = takeorder
        func._weight = weight


class filesetpredicate(_funcregistrarbase):
    """Decorator to register fileset predicate

    Usage::

        filesetpredicate = registrar.filesetpredicate()

        @filesetpredicate('mypredicate()')
        def mypredicatefunc(mctx, x):
            '''Explanation of this fileset predicate ....
            '''
            pass

    The first string argument is used also in online help.

    Optional argument 'callstatus' indicates whether a predicate
     implies 'matchctx.status()' at runtime or not (False, by
     default).

    Optional argument 'callexisting' indicates whether a predicate
    implies 'matchctx.existing()' at runtime or not (False, by
    default).

    'filesetpredicate' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'filesetpredicate' is used for
    decorating in extension.

    Otherwise, explicit 'fileset.loadpredicate()' is needed.
    """

    _getname = _funcregistrarbase._parsefuncdecl
    _docformat = "``%s``\n    %s"

    def _extrasetup(self, name, func, callstatus=False, callexisting=False):
        func._callstatus = callstatus
        func._callexisting = callexisting


class _templateregistrarbase(_funcregistrarbase):
    """Base of decorator to register functions as template specific one"""

    _docformat = ":%s: %s"


class templatekeyword(_templateregistrarbase):
    """Decorator to register template keyword

    Usage::

        templatekeyword = registrar.templatekeyword()

        @templatekeyword('mykeyword')
        def mykeywordfunc(repo, ctx, templ, cache, revcache, **args):
            '''Explanation of this template keyword ....
            '''
            pass

    The first string argument is used also in online help.

    'templatekeyword' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'templatekeyword' is used for
    decorating in extension.

    Otherwise, explicit 'templatekw.loadkeyword()' is needed.
    """


class templatefilter(_templateregistrarbase):
    """Decorator to register template filer

    Usage::

        templatefilter = registrar.templatefilter()

        @templatefilter('myfilter')
        def myfilterfunc(text):
            '''Explanation of this template filter ....
            '''
            pass

    The first string argument is used also in online help.

    'templatefilter' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'templatefilter' is used for
    decorating in extension.

    Otherwise, explicit 'templatefilters.loadkeyword()' is needed.
    """


class templatefunc(_templateregistrarbase):
    """Decorator to register template function

    Usage::

        templatefunc = registrar.templatefunc()

        @templatefunc('myfunc(arg1, arg2[, arg3])', argspec='arg1 arg2 arg3')
        def myfuncfunc(context, mapping, args):
            '''Explanation of this template function ....
            '''
            pass

    The first string argument is used also in online help.

    If optional 'argspec' is defined, the function will receive 'args' as
    a dict of named arguments. Otherwise 'args' is a list of positional
    arguments.

    'templatefunc' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'templatefunc' is used for
    decorating in extension.

    Otherwise, explicit 'templater.loadfunction()' is needed.
    """

    _getname = _funcregistrarbase._parsefuncdecl

    def _extrasetup(self, name, func, argspec=None):
        func._argspec = argspec


class internalmerge(_funcregistrarbase):
    """Decorator to register in-process merge tool

    Usage::

        internalmerge = registrar.internalmerge()

        @internalmerge('mymerge', internalmerge.mergeonly,
                       onfailure=None, precheck=None):
        def mymergefunc(repo, mynode, orig, fcd, fco, fca,
                        toolconf, files, labels=None):
            '''Explanation of this internal merge tool ....
            '''
            return 1, False # means "conflicted", "no deletion needed"

    The first string argument is used to compose actual merge tool name,
    ":name" and "internal:name" (the latter is historical one).

    The second argument is one of merge types below:

    ========== ======== ======== =========
    merge type precheck premerge fullmerge
    ========== ======== ======== =========
    nomerge     x        x        x
    mergeonly   o        x        o
    fullmerge   o        o        o
    ========== ======== ======== =========

    Optional argument 'onfailure' is the format of warning message
    to be used at failure of merging (target filename is specified
    at formatting). Or, None or so, if warning message should be
    suppressed. It can also be a function which is invoked to calculate
    the error.

    Optional argument 'precheck' is the function to be used
    before actual invocation of internal merge tool itself.
    It takes as same arguments as internal merge tool does, other than
    'files' and 'labels'. If it returns false value, merging is aborted
    immediately (and file is marked as "unresolved").

    Optional 'handlesall' indicates whether the merge tool handles all
    types of conflicts, in particular change/delete conflicts.

    'internalmerge' instance in example above can be used to
    decorate multiple functions.

    Decorated functions are registered automatically at loading
    extension, if an instance named as 'internalmerge' is used for
    decorating in extension.

    Otherwise, explicit 'filemerge.loadinternalmerge()' is needed.
    """

    _docformat = "``:%s``\n    %s"

    # merge type definitions:
    nomerge = None
    mergeonly = "mergeonly"  # just the full merge, no premerge
    fullmerge = "fullmerge"  # both premerge and merge

    def _extrasetup(
        self, name, func, mergetype, onfailure=None, precheck=None, handlesall=False
    ):
        func.mergetype = mergetype
        func.onfailure = onfailure
        func.precheck = precheck
        func.handlesall = handlesall or mergetype is self.nomerge


class hint(_funcregistrarbase):
    """Decorator to register hint messages

    Usage::

        # register a hint message
        hint = register.hint()

        @hint('next')
        def nextmsg(fromnode, tonode):
            return (_('use "hg next" to go from %s to %s')
                    % (short(fromnode), short(tonode)))

        # trigger a hint message
        def update(repo, destnode):
            wnode = repo['.'].node()
            if repo[destnode].p1().node() == wnode:
                hint.trigger('next', wnode, tonode)
    """
