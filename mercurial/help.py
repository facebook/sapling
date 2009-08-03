# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from i18n import _
import extensions, util


def moduledoc(file):
    '''return the top-level python documentation for the given file

    Loosely inspired by pydoc.source_synopsis(), but rewritten to handle \'''
    as well as """ and to return the whole text instead of just the synopsis'''
    result = []

    line = file.readline()
    while line[:1] == '#' or not line.strip():
        line = file.readline()
        if not line: break

    start = line[:3]
    if start == '"""' or start == "'''":
        line = line[3:]
        while line:
            if line.rstrip().endswith(start):
                line = line.split(start)[0]
                if line:
                    result.append(line)
                break
            elif not line:
                return None # unmatched delimiter
            result.append(line)
            line = file.readline()
    else:
        return None

    return ''.join(result)

def listexts(header, exts, maxlength):
    '''return a text listing of the given extensions'''
    if not exts:
        return ''
    result = '\n%s\n\n' % header
    for name, desc in sorted(exts.iteritems()):
        result += ' %-*s %s\n' % (maxlength + 2, ':%s:' % name, desc)
    return result

def extshelp():
    doc = _(r'''
    Mercurial has the ability to add new features through the use of
    extensions. Extensions may add new commands, add options to
    existing commands, change the default behavior of commands, or
    implement hooks.

    Extensions are not loaded by default for a variety of reasons:
    they can increase startup overhead; they may be meant for advanced
    usage only; they may provide potentially dangerous abilities (such
    as letting you destroy or modify history); they might not be ready
    for prime time; or they may alter some usual behaviors of stock
    Mercurial. It is thus up to the user to activate extensions as
    needed.

    To enable the "foo" extension, either shipped with Mercurial or in
    the Python search path, create an entry for it in your hgrc, like
    this::

      [extensions]
      foo =

    You may also specify the full path to an extension::

      [extensions]
      myfeature = ~/.hgext/myfeature.py

    To explicitly disable an extension enabled in an hgrc of broader
    scope, prepend its path with !::

      [extensions]
      # disabling extension bar residing in /path/to/extension/bar.py
      hgext.bar = !/path/to/extension/bar.py
      # ditto, but no path was supplied for extension baz
      hgext.baz = !
    ''')

    exts, maxlength = extensions.enabled()
    doc += listexts(_('enabled extensions:'), exts, maxlength)

    exts, maxlength = extensions.disabled()
    doc += listexts(_('disabled extensions:'), exts, maxlength)

    return doc

helptable = (
    (["dates"], _("Date Formats"),
     _(r'''
    Some commands allow the user to specify a date, e.g.:

    - backout, commit, import, tag: Specify the commit date.
    - log, revert, update: Select revision(s) by date.

    Many date formats are valid. Here are some examples::

      "Wed Dec 6 13:18:29 2006" (local timezone assumed)
      "Dec 6 13:18 -0600" (year assumed, time offset provided)
      "Dec 6 13:18 UTC" (UTC and GMT are aliases for +0000)
      "Dec 6" (midnight)
      "13:18" (today assumed)
      "3:39" (3:39AM assumed)
      "3:39pm" (15:39)
      "2006-12-06 13:18:29" (ISO 8601 format)
      "2006-12-6 13:18"
      "2006-12-6"
      "12-6"
      "12/6"
      "12/6/6" (Dec 6 2006)

    Lastly, there is Mercurial's internal format::

      "1165432709 0" (Wed Dec 6 13:18:29 2006 UTC)

    This is the internal representation format for dates. unixtime is
    the number of seconds since the epoch (1970-01-01 00:00 UTC).
    offset is the offset of the local timezone, in seconds west of UTC
    (negative if the timezone is east of UTC).

    The log command also accepts date ranges::

      "<{datetime}" - at or before a given date/time
      ">{datetime}" - on or after a given date/time
      "{datetime} to {datetime}" - a date range, inclusive
      "-{days}" - within a given number of days of today
    ''')),

    (["patterns"], _("File Name Patterns"),
     _(r'''
    Mercurial accepts several notations for identifying one or more
    files at a time.

    By default, Mercurial treats filenames as shell-style extended
    glob patterns.

    Alternate pattern notations must be specified explicitly.

    To use a plain path name without any pattern matching, start it
    with "path:". These path names must completely match starting at
    the current repository root.

    To use an extended glob, start a name with "glob:". Globs are
    rooted at the current directory; a glob such as "``*.c``" will
    only match files in the current directory ending with ".c".

    The supported glob syntax extensions are "``**``" to match any
    string across path separators and "{a,b}" to mean "a or b".

    To use a Perl/Python regular expression, start a name with "re:".
    Regexp pattern matching is anchored at the root of the repository.

    Plain examples::

      path:foo/bar   a name bar in a directory named foo in the root
                     of the repository
      path:path:name a file or directory named "path:name"

    Glob examples::

      glob:*.c       any name ending in ".c" in the current directory
      *.c            any name ending in ".c" in the current directory
      **.c           any name ending in ".c" in any subdirectory of the
                     current directory including itself.
      foo/*.c        any name ending in ".c" in the directory foo
      foo/**.c       any name ending in ".c" in any subdirectory of foo
                     including itself.

    Regexp examples::

      re:.*\.c$      any name ending in ".c", anywhere in the repository

    ''')),

    (['environment', 'env'], _('Environment Variables'),
     _(r'''
HG
    Path to the 'hg' executable, automatically passed when running
    hooks, extensions or external tools. If unset or empty, this is
    the hg executable's name if it's frozen, or an executable named
    'hg' (with %PATHEXT% [defaulting to COM/EXE/BAT/CMD] extensions on
    Windows) is searched.

HGEDITOR
    This is the name of the editor to run when committing. See EDITOR.

    (deprecated, use .hgrc)

HGENCODING
    This overrides the default locale setting detected by Mercurial.
    This setting is used to convert data including usernames,
    changeset descriptions, tag names, and branches. This setting can
    be overridden with the --encoding command-line option.

HGENCODINGMODE
    This sets Mercurial's behavior for handling unknown characters
    while transcoding user input. The default is "strict", which
    causes Mercurial to abort if it can't map a character. Other
    settings include "replace", which replaces unknown characters, and
    "ignore", which drops them. This setting can be overridden with
    the --encodingmode command-line option.

HGMERGE
    An executable to use for resolving merge conflicts. The program
    will be executed with three arguments: local file, remote file,
    ancestor file.

    (deprecated, use .hgrc)

HGRCPATH
    A list of files or directories to search for hgrc files. Item
    separator is ":" on Unix, ";" on Windows. If HGRCPATH is not set,
    platform default search path is used. If empty, only the .hg/hgrc
    from the current repository is read.

    For each element in HGRCPATH:

    - if it's a directory, all files ending with .rc are added
    - otherwise, the file itself will be added

HGUSER
    This is the string used as the author of a commit. If not set,
    available values will be considered in this order:

    - HGUSER (deprecated)
    - hgrc files from the HGRCPATH
    - EMAIL
    - interactive prompt
    - LOGNAME (with '@hostname' appended)

    (deprecated, use .hgrc)

EMAIL
    May be used as the author of a commit; see HGUSER.

LOGNAME
    May be used as the author of a commit; see HGUSER.

VISUAL
    This is the name of the editor to use when committing. See EDITOR.

EDITOR
    Sometimes Mercurial needs to open a text file in an editor for a
    user to modify, for example when writing commit messages. The
    editor it uses is determined by looking at the environment
    variables HGEDITOR, VISUAL and EDITOR, in that order. The first
    non-empty one is chosen. If all of them are empty, the editor
    defaults to 'vi'.

PYTHONPATH
    This is used by Python to find imported modules and may need to be
    set appropriately if this Mercurial is not installed system-wide.
    ''')),

    (['revs', 'revisions'], _('Specifying Single Revisions'),
     _(r'''
    Mercurial supports several ways to specify individual revisions.

    A plain integer is treated as a revision number. Negative integers
    are treated as sequential offsets from the tip, with -1 denoting
    the tip, -2 denoting the revision prior to the tip, and so forth.

    A 40-digit hexadecimal string is treated as a unique revision
    identifier.

    A hexadecimal string less than 40 characters long is treated as a
    unique revision identifier and is referred to as a short-form
    identifier. A short-form identifier is only valid if it is the
    prefix of exactly one full-length identifier.

    Any other string is treated as a tag or branch name. A tag name is
    a symbolic name associated with a revision identifier. A branch
    name denotes the tipmost revision of that branch. Tag and branch
    names must not contain the ":" character.

    The reserved name "tip" is a special tag that always identifies
    the most recent revision.

    The reserved name "null" indicates the null revision. This is the
    revision of an empty repository, and the parent of revision 0.

    The reserved name "." indicates the working directory parent. If
    no working directory is checked out, it is equivalent to null. If
    an uncommitted merge is in progress, "." is the revision of the
    first parent.
    ''')),

    (['mrevs', 'multirevs'], _('Specifying Multiple Revisions'),
     _(r'''
    When Mercurial accepts more than one revision, they may be
    specified individually, or provided as a topologically continuous
    range, separated by the ":" character.

    The syntax of range notation is [BEGIN]:[END], where BEGIN and END
    are revision identifiers. Both BEGIN and END are optional. If
    BEGIN is not specified, it defaults to revision number 0. If END
    is not specified, it defaults to the tip. The range ":" thus means
    "all revisions".

    If BEGIN is greater than END, revisions are treated in reverse
    order.

    A range acts as a closed interval. This means that a range of 3:5
    gives 3, 4 and 5. Similarly, a range of 9:6 gives 9, 8, 7, and 6.
    ''')),

    (['diffs'], _('Diff Formats'),
     _(r'''
    Mercurial's default format for showing changes between two
    versions of a file is compatible with the unified format of GNU
    diff, which can be used by GNU patch and many other standard
    tools.

    While this standard format is often enough, it does not encode the
    following information:

    - executable status and other permission bits
    - copy or rename information
    - changes in binary files
    - creation or deletion of empty files

    Mercurial also supports the extended diff format from the git VCS
    which addresses these limitations. The git diff format is not
    produced by default because a few widespread tools still do not
    understand this format.

    This means that when generating diffs from a Mercurial repository
    (e.g. with "hg export"), you should be careful about things like
    file copies and renames or other things mentioned above, because
    when applying a standard diff to a different repository, this
    extra information is lost. Mercurial's internal operations (like
    push and pull) are not affected by this, because they use an
    internal binary format for communicating changes.

    To make Mercurial produce the git extended diff format, use the
    --git option available for many commands, or set 'git = True' in
    the [diff] section of your hgrc. You do not need to set this
    option when importing diffs in this format or using them in the mq
    extension.
    ''')),
    (['templating', 'templates'], _('Template Usage'),
     _(r'''
    Mercurial allows you to customize output of commands through
    templates. You can either pass in a template from the command
    line, via the --template option, or select an existing
    template-style (--style).

    You can customize output for any "log-like" command: log,
    outgoing, incoming, tip, parents, heads and glog.

    Three styles are packaged with Mercurial: default (the style used
    when no explicit preference is passed), compact and changelog.
    Usage::

        $ hg log -r1 --style changelog

    A template is a piece of text, with markup to invoke variable
    expansion::

        $ hg log -r1 --template "{node}\n"
        b56ce7b07c52de7d5fd79fb89701ea538af65746

    Strings in curly braces are called keywords. The availability of
    keywords depends on the exact context of the templater. These
    keywords are usually available for templating a log-like command:

    :author:    String. The unmodified author of the changeset.
    :branches:  String. The name of the branch on which the changeset
                was committed. Will be empty if the branch name was
                default.
    :date:      Date information. The date when the changeset was
                committed.
    :desc:      String. The text of the changeset description.
    :diffstat:  String. Statistics of changes with the following
                format: "modified files: +added/-removed lines"
    :files:     List of strings. All files modified, added, or removed
                by this changeset.
    :file_adds: List of strings. Files added by this changeset.
    :file_mods: List of strings. Files modified by this changeset.
    :file_dels: List of strings. Files removed by this changeset.
    :node:      String. The changeset identification hash, as a
                40-character hexadecimal string.
    :parents:   List of strings. The parents of the changeset.
    :rev:       Integer. The repository-local changeset revision
                number.
    :tags:      List of strings. Any tags associated with the
                changeset.

    The "date" keyword does not produce human-readable output. If you
    want to use a date in your output, you can use a filter to process
    it. Filters are functions which return a string based on the input
    variable. You can also use a chain of filters to get the desired
    output::

       $ hg tip --template "{date|isodate}\n"
       2008-08-21 18:22 +0000

    List of filters:

    :addbreaks:  Any text. Add an XHTML "<br />" tag before the end of
                 every line except the last.
    :age:        Date. Returns a human-readable date/time difference
                 between the given date/time and the current
                 date/time.
    :basename:   Any text. Treats the text as a path, and returns the
                 last component of the path after splitting by the
                 path separator (ignoring trailing separators). For
                 example, "foo/bar/baz" becomes "baz" and "foo/bar//"
                 becomes "bar".
    :stripdir:   Treat the text as path and strip a directory level,
                 if possible. For example, "foo" and "foo/bar" becomes
                 "foo".
    :date:       Date. Returns a date in a Unix date format, including
                 the timezone: "Mon Sep 04 15:13:13 2006 0700".
    :domain:     Any text. Finds the first string that looks like an
                 email address, and extracts just the domain
                 component. Example: 'User <user@example.com>' becomes
                 'example.com'.
    :email:      Any text. Extracts the first string that looks like
                 an email address. Example: 'User <user@example.com>'
                 becomes 'user@example.com'.
    :escape:     Any text. Replaces the special XML/XHTML characters
                 "&", "<" and ">" with XML entities.
    :fill68:     Any text. Wraps the text to fit in 68 columns.
    :fill76:     Any text. Wraps the text to fit in 76 columns.
    :firstline:  Any text. Returns the first line of text.
    :nonempty:   Any text. Returns '(none)' if the string is empty.
    :hgdate:     Date. Returns the date as a pair of numbers:
                 "1157407993 25200" (Unix timestamp, timezone offset).
    :isodate:    Date. Returns the date in ISO 8601 format.
    :localdate:  Date. Converts a date to local date.
    :obfuscate:  Any text. Returns the input text rendered as a
                 sequence of XML entities.
    :person:     Any text. Returns the text before an email address.
    :rfc822date: Date. Returns a date using the same format used in
                 email headers.
    :short:      Changeset hash. Returns the short form of a changeset
                 hash, i.e. a 12-byte hexadecimal string.
    :shortdate:  Date. Returns a date like "2006-09-18".
    :strip:      Any text. Strips all leading and trailing whitespace.
    :tabindent:  Any text. Returns the text, with every line except
                 the first starting with a tab character.
    :urlescape:  Any text. Escapes all "special" characters. For
                 example, "foo bar" becomes "foo%20bar".
    :user:       Any text. Returns the user portion of an email
                 address.
    ''')),

    (['urls'], _('URL Paths'),
     _(r'''
    Valid URLs are of the form::

      local/filesystem/path[#revision]
      file://local/filesystem/path[#revision]
      http://[user[:pass]@]host[:port]/[path][#revision]
      https://[user[:pass]@]host[:port]/[path][#revision]
      ssh://[user[:pass]@]host[:port]/[path][#revision]

    Paths in the local filesystem can either point to Mercurial
    repositories or to bundle files (as created by 'hg bundle' or 'hg
    incoming --bundle').

    An optional identifier after # indicates a particular branch, tag,
    or changeset to use from the remote repository. See also 'hg help
    revisions'.

    Some features, such as pushing to http:// and https:// URLs are
    only possible if the feature is explicitly enabled on the remote
    Mercurial server.

    Some notes about using SSH with Mercurial:

    - SSH requires an accessible shell account on the destination
      machine and a copy of hg in the remote path or specified with as
      remotecmd.
    - path is relative to the remote user's home directory by default.
      Use an extra slash at the start of a path to specify an absolute
      path::

        ssh://example.com//tmp/repository

    - Mercurial doesn't use its own compression via SSH; the right
      thing to do is to configure it in your ~/.ssh/config, e.g.::

        Host *.mylocalnetwork.example.com
          Compression no
        Host *
          Compression yes

      Alternatively specify "ssh -C" as your ssh command in your hgrc
      or with the --ssh command line option.

    These URLs can all be stored in your hgrc with path aliases under
    the [paths] section like so::

      [paths]
      alias1 = URL1
      alias2 = URL2
      ...

    You can then use the alias for any command that uses a URL (for
    example 'hg pull alias1' would pull from the 'alias1' path).

    Two path aliases are special because they are used as defaults
    when you do not provide the URL to a command:

    default:
      When you create a repository with hg clone, the clone command
      saves the location of the source repository as the new
      repository's 'default' path. This is then used when you omit
      path from push- and pull-like commands (including incoming and
      outgoing).

    default-push:
      The push command will look for a path named 'default-push', and
      prefer it over 'default' if both are defined.
    ''')),
    (["extensions"], _("Using additional features"), extshelp),
)
