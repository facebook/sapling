# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

helptable = {
    "dates|Date Formats":
    r'''
    Some commands allow the user to specify a date:
    backout, commit, import, tag: Specify the commit date.
    log, revert, update: Select revision(s) by date.

    Many date formats are valid. Here are some examples:

    "Wed Dec 6 13:18:29 2006" (local timezone assumed)
    "Dec 6 13:18 -0600" (year assumed, time offset provided)
    "Dec 6 13:18 UTC" (UTC and GMT are aliases for +0000)
    "Dec 6" (midnight)
    "13:18" (today assumed)
    "3:39" (3:39AM assumed)
    "3:39pm" (15:39)
    "2006-12-6 13:18:29" (ISO 8601 format)
    "2006-12-6 13:18"
    "2006-12-6"
    "12-6"
    "12/6"
    "12/6/6" (Dec 6 2006)

    Lastly, there is Mercurial's internal format:

    "1165432709 0" (Wed Dec 6 13:18:29 2006 UTC)

    This is the internal representation format for dates. unixtime is
    the number of seconds since the epoch (1970-01-01 00:00 UTC). offset
    is the offset of the local timezone, in seconds west of UTC (negative
    if the timezone is east of UTC).

    The log command also accepts date ranges:

    "<{date}" - on or before a given date
    ">{date}" - on or after a given date
    "{date} to {date}" - a date range, inclusive
    "-{days}" - within a given number of days of today
    ''',

    'environment|env|Environment Variables':
    r'''
HG::
    Path to the 'hg' executable, automatically passed when running hooks,
    extensions or external tools. If unset or empty, an executable named
    'hg' (with com/exe/bat/cmd extension on Windows) is searched.

HGEDITOR::
    This is the name of the editor to use when committing. See EDITOR.

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

VISUAL::
    This is the name of the editor to use when committing. See EDITOR.

EDITOR::
    Sometimes Mercurial needs to open a text file in an editor
    for a user to modify, for example when writing commit messages.
    The editor it uses is determined by looking at the environment
    variables HGEDITOR, VISUAL and EDITOR, in that order. The first
    non-empty one is chosen. If all of them are empty, the editor
    defaults to 'vi'.

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

