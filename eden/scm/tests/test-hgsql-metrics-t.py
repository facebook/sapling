# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". '$TESTDIR/hgsql/library.sh'"
sh % "initdb"
sh % "setconfig 'extensions.treemanifest=!'"

# Populate the db with an initial commit

sh % "initclient client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "cd .."

sh % "initserver master masterrepo"

sh % "cat" << r"""
from edenscm.mercurial import extensions
from edenscm.mercurial import ui as uimod
def uisetup(ui):
    extensions.wrapfunction(uimod.ui, 'log', mylog)
def mylog(orig, self, service, *msg, **opts):
    if service in ['sqllock']:
        kwstr = ", ".join("%s=%s" % (k, v) for k, v in
                          sorted(opts.iteritems()))
        msgstr = msg[0] % msg[1:]
        self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
    return orig(self, service, *msg, **opts)
""" >> "$TESTTMP/uilog.py"
sh % "cat" << r"""
[extensions]
uilog=$TESTTMP/uilog.py
""" >> "master/.hg/hgrc"

# Verify timeouts are logged
sh % "cat" << r"""
from edenscm.mercurial import error, extensions
def uisetup(ui):
    hgsql = extensions.find('hgsql')
    extensions.wrapfunction(hgsql.sqlcontext, '__enter__', fakeenter)
def fakeenter(orig, self):
    if self.dbwritable:
        extensions.wrapfunction(self.repo.__class__, '_sqllock', lockthrow)
    return orig(self)
def lockthrow(*args, **kwargs):
    raise error.Abort("fake timeout")
""" >> "$TESTTMP/forcetimeout.py"

sh % "cp master/.hg/hgrc $TESTTMP/orighgrc"
sh % "cat" << r"""
[extensions]
forcetimeout=$TESTTMP/forcetimeout.py
""" >> "master/.hg/hgrc"
sh % "cd client"
sh % "hg push 'ssh://user@dummy/master'" == r"""
    pushing to ssh://user@dummy/master
    searching for changes
    remote: sqllock: failed to get sql lock after * seconds (glob)
    remote:  (elapsed=*, repository=$TESTTMP/master, success=false, valuetype=lockwait) (glob)
    remote: abort: fake timeout
    abort: not a Mercurial bundle
    [255]"""
sh % "cd .."
sh % "cp $TESTTMP/orighgrc master/.hg/hgrc"

# Verify sqllock times are logged
sh % "cd client"
sh % "hg push 'ssh://user@dummy/master'" == r"""
    pushing to ssh://user@dummy/master
    searching for changes
    remote: sqllock: waited for sql lock for * seconds (read 1 rows) (glob)
    remote:  (elapsed=*, repository=$TESTTMP/master, success=true, valuetype=lockwait) (glob)
    remote: adding changesets
    remote: adding manifests
    remote: adding file changes
    remote: added 1 changesets with 1 changes to 1 files
    remote: sqllock: held sql lock for * seconds (read 5 rows; write 5 rows) (glob)
    remote:  (elapsed=*, readrows=5, repository=$TESTTMP/master, valuetype=lockheld, writerows=5) (glob)"""
