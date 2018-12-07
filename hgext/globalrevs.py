# globalrevs.py
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# no-check-code

"""extension for providing strictly increasing revision numbers

With this extension enabled, Mercurial starts adding a strictly increasing
revision number to each commit which is accessible through the 'globalrev'
template.

::
    [format]
    # support strictly increasing revision numbers for new repositories.
    useglobalrevs = True

    [globalrevs]
    # Allow new commits through only pushrebase.
    onlypushrebase = True

    # In this configuration, `globalrevs` extension can only be used to query
    # strictly increasing global revision numbers already embedded in the
    # commits. In particular, `globalrevs` won't embed any data in the commits.
    readonly = True

    # Repository name to be used as key for storing global revisions data in the
    # database. If not specified, name specified through the configuration
    # `hqsql.reponame` will be used.
    reponame = customname

    # The starting global revision for a repository. We will only consider the
    # global revisions greater than equal to this value as valid global revision
    # numbers. Note that this implies there maybe commits with global revision
    # number less than this value but there is no guarantee associated those
    # numbers. Therefore, relying on global revision numbers below this value is
    # undefined behaviour.
    startrev = 0

    # If this configuration is true, the `globalrev` and `svnrev` based revsets
    # would be interoperable. In particular, the commands
    #
    #   hg log -r "svnrev(<svnrev>/<globalrev>)"
    #   hg log -r "globalrev(<svnrev>/<globalrev>)"
    #   hg log -r "r<svnrev>/r<globalrev>"
    #   hg log -r "m<svnrev>/m<globalrev>"
    #
    # would resolve to a commit with <svnrev> as the corresponding svn revision
    # number and/or <globalrev> as the corresponding strictly increasing global
    # revision number.
    svnrevinteroperation = False

"""
from __future__ import absolute_import

from mercurial import (
    error,
    extensions,
    localrepo,
    namespaces,
    registrar,
    revset,
    smartset,
)
from mercurial.i18n import _

from .hgsql import CorruptionException, executewithsql, ishgsqlbypassed, issqlrepo
from .hgsubversion import util as svnutil
from .pushrebase import isnonpushrebaseblocked


configtable = {}
configitem = registrar.configitem(configtable)
configitem("format", "useglobalrevs", default=False)
configitem("globalrevs", "onlypushrebase", default=True)
configitem("globalrevs", "readonly", default=False)
configitem("globalrevs", "reponame", default=None)
configitem("globalrevs", "startrev", default=0)
configitem("globalrevs", "svnrevinteroperation", default=False)

cmdtable = {}
command = registrar.command(cmdtable)
namespacepredicate = registrar.namespacepredicate()
revsetpredicate = registrar.revsetpredicate()
templatekeyword = registrar.templatekeyword()


@templatekeyword("globalrev")
def globalrevkw(repo, ctx, **kwargs):
    return _globalrevkw(repo, ctx, **kwargs)


def _globalrevkw(repo, ctx, **kwargs):
    grev = ctx.extra().get("global_rev")
    # If the revision number associated with the commit is before the supported
    # starting revision, nothing to do.
    if grev is not None and repo.ui.configint("globalrevs", "startrev") <= int(grev):
        return grev


cls = localrepo.localrepository
for reqs in ["_basesupported", "supportedformats"]:
    getattr(cls, reqs).add("globalrevs")


def _newreporequirementswrapper(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool("format", "useglobalrevs"):
        reqs.add("globalrevs")
    return reqs


def uisetup(ui):
    extensions.wrapfunction(
        localrepo, "newreporequirements", _newreporequirementswrapper
    )

    def _hgsqlwrapper(loaded):
        if loaded:
            hgsqlmod = extensions.find("hgsql")
            extensions.wrapfunction(hgsqlmod, "wraprepo", _sqllocalrepowrapper)

    def _hgsubversionwrapper(loaded):
        if loaded:
            hgsubversionmod = extensions.find("hgsubversion")
            extensions.wrapfunction(
                hgsubversionmod.util, "lookuprev", _lookupsvnrevwrapper
            )

            globalrevsmod = extensions.find("globalrevs")
            extensions.wrapfunction(
                globalrevsmod, "_lookupglobalrev", _lookupglobalrevwrapper
            )

    if ui.configbool("globalrevs", "svnrevinteroperation"):
        extensions.afterloaded("hgsubversion", _hgsubversionwrapper)

    # We only wrap `hgsql` extension for embedding strictly increasing global
    # revision number in commits if the repository has `hgsql` enabled and it is
    # also configured to write data to the commits. Therefore, do not wrap the
    # extension if that is not the case.
    if not ui.configbool("globalrevs", "readonly") and not ishgsqlbypassed(ui):
        extensions.afterloaded("hgsql", _hgsqlwrapper)


def reposetup(ui, repo):
    # Only need the extra functionality on the servers.
    if issqlrepo(repo):
        _validateextensions(["hgsql", "pushrebase"])
        _validaterepo(repo)


def _validateextensions(extensionlist):
    for extension in extensionlist:
        try:
            extensions.find(extension)
        except Exception:
            raise error.Abort(_("%s extension is not enabled") % extension)


def _validaterepo(repo):
    ui = repo.ui

    allowonlypushrebase = ui.configbool("globalrevs", "onlypushrebase")
    if allowonlypushrebase and not isnonpushrebaseblocked(repo):
        raise error.Abort(_("pushrebase using incorrect configuration"))


def _sqllocalrepowrapper(orig, repo):
    # This ensures that the repo is of type `sqllocalrepo` which is defined in
    # hgsql extension.
    orig(repo)

    # This class will effectively extend the `sqllocalrepo` class.
    class globalrevsrepo(repo.__class__):
        def commitctx(self, ctx, error=False):
            # Assign global revs automatically
            extra = dict(ctx.extra())
            extra["global_rev"] = self.nextrevisionnumber()
            ctx.extra = lambda: extra
            return super(globalrevsrepo, self).commitctx(ctx, error)

        def revisionnumberfromdb(self):
            # This must be executed while the SQL lock is taken
            if not self.hassqlwritelock():
                raise error.ProgrammingError("acquiring globalrev needs SQL write lock")

            reponame = self._globalrevsreponame
            cursor = self.sqlcursor

            cursor.execute(
                "SELECT value FROM revision_references "
                + "WHERE repo = %s AND "
                + "namespace = 'counter' AND "
                + "name='commit' ",
                (reponame,),
            )

            counterresults = cursor.fetchall()
            if len(counterresults) == 1:
                return int(counterresults[0][0])
            elif len(counterresults) == 0:
                raise error.Abort(
                    CorruptionException(
                        _("no commit counters for %s in database") % reponame
                    )
                )
            else:
                raise error.Abort(
                    CorruptionException(
                        _("multiple commit counters for %s in database") % reponame
                    )
                )

        def nextrevisionnumber(self):
            """ get the next strictly increasing revision number for this
            repository.
            """

            if self._nextrevisionnumber is None:
                self._nextrevisionnumber = self.revisionnumberfromdb()

            nextrev = self._nextrevisionnumber
            self._nextrevisionnumber += 1
            return nextrev

        def transaction(self, *args, **kwargs):
            tr = super(globalrevsrepo, self).transaction(*args, **kwargs)
            if tr.count > 1:
                return tr

            def transactionabort(orig):
                self._nextrevisionnumber = None
                return orig()

            extensions.wrapfunction(tr, "_abort", transactionabort)
            return tr

        def _updaterevisionreferences(self, *args, **kwargs):
            super(globalrevsrepo, self)._updaterevisionreferences(*args, **kwargs)

            newcount = self._nextrevisionnumber

            # Only write to database if the global revision number actually
            # changed.
            if newcount is not None:
                reponame = self._globalrevsreponame
                cursor = self.sqlcursor

                cursor.execute(
                    "UPDATE revision_references "
                    + "SET value=%s "
                    + "WHERE repo=%s AND namespace='counter' AND name='commit'",
                    (newcount, reponame),
                )

    repo._globalrevsreponame = (
        repo.ui.config("globalrevs", "reponame") or repo.sqlreponame
    )
    repo._nextrevisionnumber = None
    repo.__class__ = globalrevsrepo


def _lookupsvnrevwrapper(orig, repo, rev):
    return _lookuprev(orig, _lookupglobalrev, repo, rev)


def _lookupglobalrevwrapper(orig, repo, rev):
    return _lookuprev(svnutil.lookuprev, orig, repo, rev)


def _lookuprev(svnrevlookupfunc, globalrevlookupfunc, repo, rev):
    # If the revision number being looked up is before the supported starting
    # global revision, try if it works as a svn revision number.
    lookupfunc = (
        svnrevlookupfunc
        if (repo.ui.configint("globalrevs", "startrev") > rev)
        else globalrevlookupfunc
    )
    return lookupfunc(repo, rev)


def _lookupglobalrev(repo, grev):
    # If the revision number being looked up is before the supported starting
    # global revision, nothing to do.
    if repo.ui.configint("globalrevs", "startrev") > grev:
        return []

    cl = repo.changelog
    changelogrevision = cl.changelogrevision
    tonode = cl.node

    def matchglobalrev(rev):
        commitglobalrev = changelogrevision(rev).extra.get("global_rev")
        return commitglobalrev is not None and int(commitglobalrev) == grev

    matchedrevs = []
    for rev in repo.revs("reverse(all())"):
        if matchglobalrev(rev):
            matchedrevs.append(tonode(rev))
            break

    return matchedrevs


def _lookupname(repo, name):
    if name.startswith("m") and name[1:].isdigit():
        return _lookupglobalrev(repo, int(name[1:]))


@namespacepredicate("globalrevs", priority=75)
def _getnamespace(_repo):
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_lookupname, nodemap=lambda repo, node: []
    )


@revsetpredicate("globalrev(number)", safe=True, weight=10)
def _revsetglobalrev(repo, subset, x):
    """Changesets with given global revision number.
    """
    args = revset.getargs(x, 1, 1, "globalrev takes one argument")
    globalrev = revset.getinteger(
        args[0], "the argument to globalrev() must be a number"
    )

    return subset & smartset.baseset(_lookupglobalrev(repo, globalrev))


@command("^globalrev", [], _("hg globalrev"))
def globalrev(ui, repo, *args, **opts):
    """prints out the next global revision number for a particular repository by
    reading it from the database.
    """

    if not issqlrepo(repo):
        raise error.Abort(_("this repository is not a sql backed repository"))

    def _printnextglobalrev():
        ui.status(_("%s\n") % repo.revisionnumberfromdb())

    executewithsql(repo, _printnextglobalrev, sqllock=True)


@command(
    "^initglobalrev",
    [
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run initglobalrev if you know exactly what you're doing"),
        )
    ],
    _("hg initglobalrev START"),
)
def initglobalrev(ui, repo, start, *args, **opts):
    """ initializes the global revision number for a particular repository by
    writing it to the database.
    """

    if not issqlrepo(repo):
        raise error.Abort(_("this repository is not a sql backed repository"))

    if not opts.get("i_know_what_i_am_doing"):
        raise error.Abort(
            _(
                "You must pass --i-know-what-i-am-doing to run this command. "
                + "Only the Mercurial server admins should ever run this."
            )
        )

    try:
        startrev = int(start)
    except ValueError:
        raise error.Abort(_("start must be an integer."))

    def _initglobalrev():
        cursor = repo.sqlcursor
        reponame = repo._globalrevsreponame

        # Our schemas are setup such that this query will fail if we try to
        # update an existing row which is exactly what we desire here.
        cursor.execute(
            "INSERT INTO "
            + "revision_references(repo, namespace, name, value) "
            + "VALUES(%s, 'counter', 'commit', %s)",
            (reponame, startrev),
        )

        repo.sqlconn.commit()

    executewithsql(repo, _initglobalrev, sqllock=True)
