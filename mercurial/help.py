# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

helptable = {
    "dates|Date Formats":
    r'''
    Some commands (backout, commit, tag) allow the user to specify a date.
    Possible formats for dates are:

YYYY-mm-dd \HH:MM[:SS] [(+|-)NNNN]::
    This is a subset of ISO 8601, allowing just the recommended notations
    for date and time. The last part represents the timezone; if omitted,
    local time is assumed. Examples:

    "2005-08-22 03:27 -0700"

    "2006-04-19 21:39:51"

aaa bbb dd HH:MM:SS YYYY [(+|-)NNNN]::
    This is the date format used by the C library. Here, aaa stands for
    abbreviated weekday name and bbb for abbreviated month name. The last
    part represents the timezone; if omitted, local time is assumed.
    Examples:

    "Mon Aug 22 03:27:00 2005 -0700"

    "Wed Apr 19 21:39:51 2006"

unixtime offset::
    This is the internal representation format for dates. unixtime is
    the number of seconds since the epoch (1970-01-01 00:00 UTC). offset
    is the offset of the local timezone, in seconds west of UTC (negative
    if the timezone is east of UTC).
    Examples:

    "1124706420 25200" (2005-08-22 03:27:00 -0700)

    "1145475591 -7200" (2006-04-19 21:39:51 +0200)
    ''',

    'environment|env|Environment Variables':
    r'''
HGEDITOR::
    This is the name of the editor to use when committing. Defaults to the
    value of EDITOR.

    (deprecated, use .hgrc)

HGENCODING::
    This overrides the default locale setting detected by Mercurial.
    This setting is used to convert data including usernames,
    changeset descriptions, tag names, and branches. This setting can
    be overridden with the --encoding command-line option.

HGENCODINGMODE::
    This sets Mercurial's behavior for handling unknown characters
    while transcoding user inputs. The default is "strict", which
    causes Mercurial to abort if it can't translate a character. Other
    settings include "replace", which replaces unknown characters, and
    "ignore", which drops them. This setting can be overridden with
    the --encodingmode command-line option.

HGMERGE::
    An executable to use for resolving merge conflicts. The program
    will be executed with three arguments: local file, remote file,
    ancestor file.

    The default program is "hgmerge", which is a shell script provided
    by Mercurial with some sensible defaults.

    (deprecated, use .hgrc)

HGRCPATH::
    A list of files or directories to search for hgrc files.  Item
    separator is ":" on Unix, ";" on Windows.  If HGRCPATH is not set,
    platform default search path is used.  If empty, only .hg/hgrc of
    current repository is read.

    For each element in path, if a directory, all entries in directory
    ending with ".rc" are added to path.  Else, element itself is
    added to path.

HGUSER::
    This is the string used for the author of a commit.

    (deprecated, use .hgrc)

EMAIL::
    If HGUSER is not set, this will be used as the author for a commit.

LOGNAME::
    If neither HGUSER nor EMAIL is set, LOGNAME will be used (with
    '@hostname' appended) as the author value for a commit.

EDITOR::
    This is the name of the editor used in the hgmerge script. It will be
    used for commit messages if HGEDITOR isn't set. Defaults to 'vi'.

PYTHONPATH::
    This is used by Python to find imported modules and may need to be set
    appropriately if Mercurial is not installed system-wide.
    ''',

    "patterns|File Name Patterns": r'''
    Mercurial accepts several notations for identifying one or more
    files at a time.

    By default, Mercurial treats filenames as shell-style extended
    glob patterns.

    Alternate pattern notations must be specified explicitly.

    To use a plain path name without any pattern matching, start a
    name with "path:".  These path names must match completely, from
    the root of the current repository.

    To use an extended glob, start a name with "glob:".  Globs are
    rooted at the current directory; a glob such as "*.c" will match
    files ending in ".c" in the current directory only.

    The supported glob syntax extensions are "**" to match any string
    across path separators, and "{a,b}" to mean "a or b".

    To use a Perl/Python regular expression, start a name with "re:".
    Regexp pattern matching is anchored at the root of the repository.

    Plain examples:

    path:foo/bar   a name bar in a directory named foo in the root of
                   the repository
    path:path:name a file or directory named "path:name"

    Glob examples:

    glob:*.c       any name ending in ".c" in the current directory
    *.c            any name ending in ".c" in the current directory
    **.c           any name ending in ".c" in the current directory, or
                   any subdirectory
    foo/*.c        any name ending in ".c" in the directory foo
    foo/**.c       any name ending in ".c" in the directory foo, or any
                   subdirectory

    Regexp examples:

    re:.*\.c$      any name ending in ".c", anywhere in the repository

''',
}

