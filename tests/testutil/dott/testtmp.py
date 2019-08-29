# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""$TESTTMP support for .t tests.

Importing this module has side effect of setting up $TESTTMP.
"""

from __future__ import absolute_import

import atexit
import os
import shutil
import sys
import tempfile

from . import shlib
from .shobj import normalizeoutput


def _setuptesttmp():
    """Prepare the temporary directory. Return TESTTMP and HGRCPATH.

    This is for compatibility of auto-translated .t tests.
    New tests should use a different API that uses a context manager.
    """
    testtmp = os.environ.get("TESTTMP")
    hgrcpath = os.environ.get("HGRCPATH")
    if not (testtmp and hgrcpath):
        # Create new TESTTMP and HGRCPATH
        path = tempfile.mkdtemp(prefix="hgtest")
        if any(arg.startswith("--keep") for arg in sys.argv):
            atexit.register(sys.stderr.write, "Keeping tmpdir: %s\n" % path)
        else:
            atexit.register(shutil.rmtree, path, True)

        hgrcpath = os.path.join(path, ".hgrc")
        testtmp = os.path.join(path, "testtmp")
        shlib.mkdir(testtmp)
        os.chdir(testtmp)

    @normalizeoutput
    def replacetesttmp(out, testtmp=testtmp):
        return out.replace(testtmp, "$TESTTMP")

    # See _getenv from run-tests.py
    os.environ.update(
        {
            "COLUMNS": "80",
            "EMAIL": "Foo Bar <foo.bar@example.com>",
            "HGCOLORS": "16",
            "HGEDITOR": "true",
            "HGMERGE": "internal:merge",
            "HGRCPATH": hgrcpath,
            "HGUSER": "test",
            "HOME": testtmp,
            "LANG": "C",
            "LANGUAGE": "C",
            "LC_ALL": "C",
            "TESTTMP": testtmp,
            "TESTDIR": shlib.TESTDIR,
            "TZ": "GMT",
        }
    )
    open(hgrcpath, "w").write(
        """
[ui]
slash = True
interactive = False
mergemarkers = detailed
promptecho = True

[defaults]

[devel]
all-warnings = true
default-date = 0 0

[lfs]

[web]
address = localhost

[extensions]
treemanifest=

[treemanifest]
flatcompat=True

[remotefilelog]
reponame=reponame-default
cachepath=$TESTTMP/default-hgcache

[commands]
names=absorb|sf add addremove amend annotate|blame archive backfillmanifestrevlog backfilltree backout backupdelete backupdisable backupenable bisect blackbox bookmarks|bookmark bottom branch bundle cachemanifest cat cherry-pick chistedit clone cloud commit|ci config|showconfig|debugconfig copy|cp crecord diff export files fold|squash foo forget fs fsl fssl gc getavailablebackups githelp|git graft grep heads help hide|strip hint histedit histgrep identify|id import|patch incoming|in init isbackedup journal locate log|history manifest merge metaedit metaedit next odiff outgoing|out parents pasterage paths phase prefetch previous prune pull pullbackup purge|clean push pushbackup rage rebase record recover redo reflog remove|rm rename|move|mv repack reset resolve restack revert rollback root sb sba serve share shelve shortlog show sl smartlog|slog sparse split ssl stash status|st summary|sum svn tag tags tip top unamend unbundle uncommit undo unhide unshare unshelve update|up|checkout|co verify verifyremotefilelog version wgrep
"""
    )
    return testtmp, hgrcpath


TESTTMP, HGRCPATH = _setuptesttmp()
