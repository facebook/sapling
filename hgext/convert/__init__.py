# convert.py Foreign SCM converter
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
'''converting foreign VCS repositories to Mercurial'''

import convcmd
import cvsps
from mercurial import commands
from mercurial.i18n import _

# Commands definition was moved elsewhere to ease demandload job.

def convert(ui, src, dest=None, revmapfile=None, **opts):
    """convert a foreign SCM repository to a Mercurial one.

    Accepted source formats [identifiers]:
    - Mercurial [hg]
    - CVS [cvs]
    - Darcs [darcs]
    - git [git]
    - Subversion [svn]
    - Monotone [mtn]
    - GNU Arch [gnuarch]
    - Bazaar [bzr]

    Accepted destination formats [identifiers]:
    - Mercurial [hg]
    - Subversion [svn] (history on branches is not preserved)

    If no revision is given, all revisions will be converted. Otherwise,
    convert will only import up to the named revision (given in a format
    understood by the source).

    If no destination directory name is specified, it defaults to the
    basename of the source with '-hg' appended.  If the destination
    repository doesn't exist, it will be created.

    If <REVMAP> isn't given, it will be put in a default location
    (<dest>/.hg/shamap by default).  The <REVMAP> is a simple text
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

    --config convert.hg.ignoreerrors=False (boolean)
        ignore integrity errors when reading. Use it to fix Mercurial
        repositories with missing revlogs, by converting from and to
        Mercurial.
    --config convert.hg.saverev=False          (boolean)
        store original revision ID in changeset (forces target IDs to change)
    --config convert.hg.startrev=0            (hg revision identifier)
        convert start revision and its descendants

    CVS Source
    ----------

    CVS source will use a sandbox (i.e. a checked-out copy) from CVS
    to indicate the starting point of what will be converted. Direct
    access to the repository files is not needed, unless of course
    the repository is :local:. The conversion uses the top level
    directory in the sandbox to find the CVS repository, and then uses
    CVS rlog commands to find files to convert. This means that unless
    a filemap is given, all files under the starting directory will be
    converted, and that any directory reorganisation in the CVS
    sandbox is ignored.

    Because CVS does not have changesets, it is necessary to collect
    individual commits to CVS and merge them into changesets. CVS
    source uses its internal changeset merging code by default but can
    be configured to call the external 'cvsps' program by setting:
        --config convert.cvsps='cvsps -A -u --cvs-direct -q'
    This is a legacy option and may be removed in future.

    The options shown are the defaults.

    Internal cvsps is selected by setting
        --config convert.cvsps=builtin
    and has a few more configurable options:
        --config convert.cvsps.fuzz=60   (integer)
            Specify the maximum time (in seconds) that is allowed between
            commits with identical user and log message in a single
            changeset. When very large files were checked in as part
            of a changeset then the default may not be long enough.
        --config convert.cvsps.mergeto='{{mergetobranch ([-\w]+)}}'
            Specify a regular expression to which commit log messages are
            matched. If a match occurs, then the conversion process will
            insert a dummy revision merging the branch on which this log
            message occurs to the branch indicated in the regex.
        --config convert.cvsps.mergefrom='{{mergefrombranch ([-\w]+)}}'
            Specify a regular expression to which commit log messages are
            matched. If a match occurs, then the conversion process will
            add the most recent revision on the branch indicated in the
            regex as the second parent of the changeset.

    The hgext/convert/cvsps wrapper script allows the builtin changeset
    merging code to be run without doing a conversion. Its parameters and
    output are similar to that of cvsps 2.1.

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

    Source history can be retrieved starting at a specific revision,
    instead of being integrally converted. Only single branch
    conversions are supported.

    --config convert.svn.startrev=0           (svn revision number)
        specify start Subversion revision.

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

def debugcvsps(ui, *args, **opts):
    '''create changeset information from CVS

    This command is intended as a debugging tool for the CVS to Mercurial
    converter, and can be used as a direct replacement for cvsps.

    Hg debugcvsps reads the CVS rlog for current directory (or any named
    directory) in the CVS repository, and converts the log to a series of
    changesets based on matching commit log entries and dates.'''
    return cvsps.debugcvsps(ui, *args, **opts)

commands.norepo += " convert debugsvnlog debugcvsps"

cmdtable = {
    "convert":
        (convert,
         [('A', 'authors', '', _('username mapping filename')),
          ('d', 'dest-type', '', _('destination repository type')),
          ('', 'filemap', '', _('remap file names using contents of file')),
          ('r', 'rev', '', _('import up to target revision REV')),
          ('s', 'source-type', '', _('source repository type')),
          ('', 'splicemap', '', _('splice synthesized history into place')),
          ('', 'datesort', None, _('try to sort changesets by date'))],
         _('hg convert [OPTION]... SOURCE [DEST [REVMAP]]')),
    "debugsvnlog":
        (debugsvnlog,
         [],
         'hg debugsvnlog'),
    "debugcvsps":
        (debugcvsps,
         [
          # Main options shared with cvsps-2.1
          ('b', 'branches', [], _('only return changes on specified branches')),
          ('p', 'prefix', '', _('prefix to remove from file names')),
          ('r', 'revisions', [], _('only return changes after or between specified tags')),
          ('u', 'update-cache', None, _("update cvs log cache")),
          ('x', 'new-cache', None, _("create new cvs log cache")),
          ('z', 'fuzz', 60, _('set commit time fuzz in seconds')),
          ('', 'root', '', _('specify cvsroot')),
          # Options specific to builtin cvsps
          ('', 'parents', '', _('show parent changesets')),
          ('', 'ancestors', '', _('show current changeset in ancestor branches')),
          # Options that are ignored for compatibility with cvsps-2.1
          ('A', 'cvs-direct', None, _('ignored for compatibility')),
         ],
         _('hg debugcvsps [OPTION]... [PATH]...')),
}
