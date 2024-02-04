# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

    # If this configuration is true, we use ScmQuery to lookup the mapping from
    # `globalrev->hash` to enable fast lookup of the commits based on the
    # globalrev. This configuration is only effective on the clients.
    scmquerylookup = False
"""
from __future__ import absolute_import

from sapling import error, extensions, localrepo, namespaces, registrar, revset
from sapling.i18n import _
from sapling.namespaces import namespace

from .hgsql import CorruptionException, executewithsql, ishgsqlbypassed, issqlrepo
from .pushrebase import isnonpushrebaseblocked


configtable = {}
configitem = registrar.configitem(configtable)
configitem("format", "useglobalrevs", default=False)
configitem("globalrevs", "onlypushrebase", default=True)
configitem("globalrevs", "readonly", default=False)
configitem("globalrevs", "reponame", default=None)
configitem("globalrevs", "scmquerylookup", default=False)
configitem("globalrevs", "edenapilookup", default=False)
configitem("globalrevs", "startrev", default=0)

cmdtable = {}
command = registrar.command(cmdtable)
namespacepredicate = registrar.namespacepredicate()
revsetpredicate = registrar.revsetpredicate()
templatekeyword = registrar.templatekeyword()

EXTRASCONVERTKEY = "convert_revision"
EXTRASGLOBALREVKEY = "global_rev"


@templatekeyword("globalrev")
@templatekeyword("svnrev")
def globalrevkw(repo, ctx, **kwargs):
    return _getglobalrev(repo.ui, ctx.extra())


def _newreporequirementswrapper(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool("format", "useglobalrevs"):
        reqs.add("globalrevs")
    return reqs


def uisetup(ui) -> None:
    extensions.wrapfunction(
        localrepo, "newreporequirements", _newreporequirementswrapper
    )

    def _hgsqlwrapper(loaded):
        if loaded:
            hgsqlmod = extensions.find("hgsql")
            extensions.wrapfunction(hgsqlmod, "wraprepo", _sqllocalrepowrapper)

    # We only wrap `hgsql` extension for embedding strictly increasing global
    # revision number in commits if the repository has `hgsql` enabled and it is
    # also configured to write data to the commits. Therefore, do not wrap the
    # extension if that is not the case.
    if not ui.configbool("globalrevs", "readonly") and not ishgsqlbypassed(ui):
        extensions.afterloaded("hgsql", _hgsqlwrapper)

    cls = localrepo.localrepository
    for reqs in ["_basesupported", "supportedformats"]:
        getattr(cls, reqs).add("globalrevs")


def reposetup(ui, repo) -> None:
    # Only need the extra functionality on the servers.
    if issqlrepo(repo):
        _validateextensions(["hgsql", "pushrebase"])
        _validaterepo(repo)


def _validateextensions(extensionlist) -> None:
    for extension in extensionlist:
        try:
            extensions.find(extension)
        except Exception:
            raise error.Abort(_("%s extension is not enabled") % extension)


def _validaterepo(repo) -> None:
    ui = repo.ui

    allowonlypushrebase = ui.configbool("globalrevs", "onlypushrebase")
    if allowonlypushrebase and not isnonpushrebaseblocked(repo):
        raise error.Abort(_("pushrebase using incorrect configuration"))


def _sqllocalrepowrapper(orig, repo) -> None:
    # This ensures that the repo is of type `sqllocalrepo` which is defined in
    # hgsql extension.
    orig(repo)

    if not extensions.isenabled(repo.ui, "globalrevs"):
        return

    # This class will effectively extend the `sqllocalrepo` class.
    class globalrevsrepo(repo.__class__):
        def commitctx(self, ctx, error=False):
            # Assign global revs automatically
            extra = dict(ctx.extra())
            extra[EXTRASGLOBALREVKEY] = str(self.nextrevisionnumber())
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
            """get the next strictly increasing revision number for this
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


def _lookupglobalrev(repo, grev):
    # A `globalrev` < 0 will never resolve to any commit.
    if grev < 0:
        return []

    cl = repo.changelog
    changelogrevision = cl.changelogrevision
    tonode = cl.node
    ui = repo.ui

    useedenapi = ui.configbool("globalrevs", "edenapilookup")
    if useedenapi and repo.nullableedenapi is not None:
        rsp = list(repo.edenapi.committranslateids([{"Globalrev": grev}], "Hg"))
        if rsp:
            hgnode = rsp[0]["translated"]["Hg"]
            return [hgnode]
        elif ui.configbool("globalrevs", "edenapi-authoritative", True):
            return []

    for rev in repo.revs("reverse(public())").prefetch("text"):
        commitextra = changelogrevision(rev).extra

        # _getglobalrev returns "globalrev or svnrev"
        globalrev = _getglobalrev(ui, commitextra)
        if globalrev is None:
            continue

        globalrev = int(globalrev)
        if globalrev == grev:
            return [tonode(rev)]
        elif globalrev < grev:
            # globalrev is always bigger than svnrev if both are present.
            # globalrev will only get smaller from here on out, so we can
            # return early.
            return []

        # In case commit has both globalrev and svnrev, we already
        # checked globalrev above, so now we need to check svnrev
        # directly.
        svnrev = _getsvnrev(commitextra)
        if svnrev is not None and int(svnrev) == grev:
            return [tonode(rev)]

    return []


def _lookupname(repo, name):
    if (name.startswith("m") or name.startswith("r")) and name[1:].isdigit():
        return _lookupglobalrev(repo, int(name[1:]))


@namespacepredicate("globalrevs", priority=75)
def _getnamespace(_repo) -> namespace:
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_lookupname, nodemap=lambda repo, node: []
    )


@revsetpredicate("globalrev(number)", safe=True, weight=10)
def _revsetglobalrev(repo, subset, x):
    """Changesets with given global revision number."""
    args = revset.getargs(x, 1, 1, "globalrev takes one argument")
    globalrev = revset.getinteger(
        args[0], "the argument to globalrev() must be a number"
    )

    torev = repo.changelog.rev
    revs = revset.baseset(
        (torev(n) for n in _lookupglobalrev(repo, globalrev)), repo=repo
    )
    return subset & revs


@revsetpredicate("svnrev(number)", safe=True, weight=10)
def _revsetsvnrev(repo, subset, x):
    """Changesets with given Subversion revision number."""
    args = revset.getargs(x, 1, 1, "svnrev takes one argument")
    svnrev = revset.getinteger(args[0], "the argument to svnrev() must be a number")

    torev = repo.changelog.rev
    revs = revset.baseset((torev(n) for n in _lookupglobalrev(repo, svnrev)), repo=repo)
    return subset & revs


def getglobalrev(ui, ctx, defval=None):
    """Wrapper around _getglobalrev. See _getglobalrev for more detail."""
    grev = _getglobalrev(ui, ctx.extra())
    if grev:
        return grev
    return defval


def _getglobalrev(ui, commitextra):
    grev = commitextra.get(EXTRASGLOBALREVKEY)

    # If we did not find `globalrev` in the commit extras, lets also look for
    # the `svnrev` in the commit extras before we give up. Also, do not return
    # the `globalrev` if it is before the supported starting revision.
    return (
        _getsvnrev(commitextra)
        if not grev or ui.configint("globalrevs", "startrev") > int(grev)
        else grev
    )


def _getsvnrev(commitextra):
    convertrev = commitextra.get(EXTRASCONVERTKEY)

    # ex. svn:uuid/path@1234
    if convertrev and "svn:" in convertrev:
        return convertrev.rsplit("@", 1)[-1]


@command("globalrev", [], _("@prog@ globalrev"))
def globalrev(ui, repo, *args, **opts) -> None:
    """prints out the next global revision number for a particular repository by
    reading it from the database.
    """

    if not issqlrepo(repo):
        raise error.Abort(_("this repository is not a sql backed repository"))

    def _printnextglobalrev():
        ui.status(_("%s\n") % repo.revisionnumberfromdb())

    executewithsql(repo, _printnextglobalrev, sqllock=True)


@command(
    "initglobalrev",
    [
        (
            "",
            "i-know-what-i-am-doing",
            None,
            _("only run initglobalrev if you know exactly what you're doing"),
        )
    ],
    _("@prog@ initglobalrev START"),
)
def initglobalrev(ui, repo, start, *args, **opts) -> None:
    """initializes the global revision number for a particular repository by
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
