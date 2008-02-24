# convert.py Foreign SCM converter
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import convcmd
from mercurial import commands

# Commands definition was moved elsewhere to ease demandload job.

def convert(ui, src, dest=None, revmapfile=None, **opts):
    """Convert a foreign SCM repository to a Mercurial one.

    Accepted source formats:
    - Mercurial
    - CVS
    - Darcs
    - git
    - Subversion
    - GNU Arch

    Accepted destination formats:
    - Mercurial
    - Subversion (history on branches is not preserved)

    If no revision is given, all revisions will be converted. Otherwise,
    convert will only import up to the named revision (given in a format
    understood by the source).

    If no destination directory name is specified, it defaults to the
    basename of the source with '-hg' appended.  If the destination
    repository doesn't exist, it will be created.

    If <MAPFILE> isn't given, it will be put in a default location
    (<dest>/.hg/shamap by default).  The <MAPFILE> is a simple text
    file that maps each source commit ID to the destination ID for
    that revision, like so:
    <source ID> <destination ID>

    If the file doesn't exist, it's automatically created.  It's updated
    on each commit copied, so convert-repo can be interrupted and can
    be run repeatedly to copy new commits.

    The [username mapping] file is a simple text file that maps each source
    commit author to a destination commit author. It is handy for source SCMs
    that use unix logins to identify authors (eg: CVS). One line per author
    mapping and the line format is:
    srcauthor=whatever string you want

    The filemap is a file that allows filtering and remapping of files
    and directories.  Comment lines start with '#'.  Each line can
    contain one of the following directives:

      include path/to/file

      exclude path/to/file

      rename from/file to/file

    The 'include' directive causes a file, or all files under a
    directory, to be included in the destination repository, and the
    exclusion of all other files and dirs not explicitely included.
    The 'exclude' directive causes files or directories to be omitted.
    The 'rename' directive renames a file or directory.  To rename from a
    subdirectory into the root of the repository, use '.' as the path to
    rename to.

    The splicemap is a file that allows insertion of synthetic
    history, letting you specify the parents of a revision.  This is
    useful if you want to e.g. give a Subversion merge two parents, or
    graft two disconnected series of history together.  Each entry
    contains a key, followed by a space, followed by one or two
    values, separated by spaces.  The key is the revision ID in the
    source revision control system whose parents should be modified
    (same format as a key in .hg/shamap).  The values are the revision
    IDs (in either the source or destination revision control system)
    that should be used as the new parents for that node.

    Mercurial Source
    -----------------

    --config convert.hg.saverev=True          (boolean)
        allow target to preserve source revision ID

    Subversion Source
    -----------------

    Subversion source detects classical trunk/branches/tags layouts.
    By default, the supplied "svn://repo/path/" source URL is
    converted as a single branch. If "svn://repo/path/trunk" exists
    it replaces the default branch. If "svn://repo/path/branches"
    exists, its subdirectories are listed as possible branches. If
    "svn://repo/path/tags" exists, it is looked for tags referencing
    converted branches. Default "trunk", "branches" and "tags" values
    can be overriden with following options. Set them to paths
    relative to the source URL, or leave them blank to disable
    autodetection.

    --config convert.svn.branches=branches    (directory name)
        specify the directory containing branches
    --config convert.svn.tags=tags            (directory name)
        specify the directory containing tags
    --config convert.svn.trunk=trunk          (directory name)
        specify the name of the trunk branch

    Mercurial Destination
    ---------------------

    --config convert.hg.clonebranches=False   (boolean)
        dispatch source branches in separate clones.
    --config convert.hg.tagsbranch=default    (branch name)
        tag revisions branch name
    --config convert.hg.usebranchnames=True   (boolean)
        preserve branch names

    """
    return convcmd.convert(ui, src, dest, revmapfile, **opts)

def debugsvnlog(ui, **opts):
    return convcmd.debugsvnlog(ui, **opts)

commands.norepo += " convert debugsvnlog"

cmdtable = {
    "convert":
        (convert,
         [('A', 'authors', '', 'username mapping filename'),
          ('d', 'dest-type', '', 'destination repository type'),
          ('', 'filemap', '', 'remap file names using contents of file'),
          ('r', 'rev', '', 'import up to target revision REV'),
          ('s', 'source-type', '', 'source repository type'),
          ('', 'splicemap', '', 'splice synthesized history into place'),
          ('', 'datesort', None, 'try to sort changesets by date')],
         'hg convert [OPTION]... SOURCE [DEST [MAPFILE]]'),
    "debugsvnlog":
        (debugsvnlog,
         [],
         'hg debugsvnlog'),
}
