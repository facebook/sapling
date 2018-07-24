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
"""

from hgsql import CorruptionException, executewithsql, ishgsqlbypassed, issqlrepo
from mercurial import error, extensions, localrepo, registrar
from mercurial.i18n import _
from pushrebase import isnonpushrebaseblocked


configtable = {}
configitem = registrar.configitem(configtable)
configitem("format", "useglobalrevs", default=False)
configitem("globalrevs", "onlypushrebase", default=True)

cmdtable = {}
command = registrar.command(cmdtable)
templatekeyword = registrar.templatekeyword()


@templatekeyword("globalrev")
def _globalrevkw(repo, ctx, **args):
    return ctx.extra().get("global_rev")


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

    def _pushrebasewrapper(loaded):
        if loaded:
            pushrebasemod = extensions.find("pushrebase")
            extensions.wrapfunction(pushrebasemod, "_commit", _commitwrapper)

    def _hgsqlwrapper(loaded):
        if loaded:
            hgsqlmod = extensions.find("hgsql")
            extensions.wrapfunction(hgsqlmod, "wraprepo", _sqllocalrepowrapper)

    def _hgsubversionwrapper(loaded):
        if loaded:
            hgsubversionmod = extensions.find("hgsubversion")
            extensions.wrapfunction(
                hgsubversionmod.svnrepo, "generate_repo_class", _svnlocalrepowrapper
            )

    extensions.afterloaded("pushrebase", _pushrebasewrapper)
    extensions.afterloaded("hgsql", _hgsqlwrapper)
    extensions.afterloaded("hgsubversion", _hgsubversionwrapper)


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

    if ishgsqlbypassed(ui):
        raise error.Abort(_("hgsql using incorrect configuration"))

    allowonlypushrebase = ui.configbool("globalrevs", "onlypushrebase")
    if allowonlypushrebase and not isnonpushrebaseblocked(repo):
        raise error.Abort(_("pushrebase using incorrect configuration"))


def _sqllocalrepowrapper(orig, repo):
    # This ensures that the repo is of type `sqllocalrepo` which is defined in
    # hgsql extension.
    orig(repo)

    # This class will effectively extend the `sqllocalrepo` class.
    class globalrevsrepo(repo.__class__):
        def revisionnumberfromdb(self):
            reponame = self.sqlreponame
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

        def invalidate(self, *args, **kwargs):
            super(globalrevsrepo, self).invalidate(*args, **kwargs)
            self._nextrevisionnumber = None

        def transaction(self, *args, **kwargs):
            tr = super(globalrevsrepo, self).transaction(*args, **kwargs)

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
                reponame = self.sqlreponame
                cursor = self.sqlcursor

                cursor.execute(
                    "UPDATE revision_references "
                    + "SET value=%s "
                    + "WHERE repo=%s AND namespace='counter' AND name='commit'",
                    (newcount, reponame),
                )

    repo._nextrevisionnumber = None
    repo.__class__ = globalrevsrepo


def _svnlocalrepowrapper(orig, ui, repo):
    # This ensures that the repo is of type `svnlocalrepo` which is defined in
    # hgsubversion extension.
    orig(ui, repo)

    # Only need the extra functionality on the servers.
    if issqlrepo(repo):
        # This class will effectively extend the `svnlocalrepo` class.
        class globalrevssvnlocalrepo(repo.__class__):
            def svn_commitctx(self, ctx):
                with repo.wlock(), repo.lock(), repo.transaction("svncommit"):
                    extras = ctx.extra()
                    extras["global_rev"] = repo.nextrevisionnumber()
                    return super(globalrevssvnlocalrepo, self).svn_commitctx(ctx)

        repo.__class__ = globalrevssvnlocalrepo


def _commitwrapper(orig, repo, parents, desc, files, filectx, user, date, extras):
    # Only need the extra functionality on the servers.
    if issqlrepo(repo):
        extras["global_rev"] = repo.nextrevisionnumber()

    return orig(repo, parents, desc, files, filectx, user, date, extras)


@command("^globalrev", [], _("hg globalrev"))
def globalrev(ui, repo, *args, **opts):
    """prints out the next global revision number for a particular repository by
    reading it from the database.
    """

    if not issqlrepo(repo):
        raise error.Abort(_("this repository is not a sql backed repository"))

    def _printnextglobalrev():
        ui.status(_("%s\n") % repo.revisionnumberfromdb())

    executewithsql(repo, _printnextglobalrev, sqllock=False)


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
        reponame = repo.sqlreponame

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
