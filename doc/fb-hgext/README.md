fb-hgext
=============

This is a collection of Mercurial extensions written at Facebook. Many of them
are in heavy use by thousands of users on a daily basis. However, some of them
are very specific to Facebook's very large repositories so their value to
others will vary. We're still making these open source as examples of the
workflows we use and direction we are working.

Note that there will be extensions included here that only work with Facebook's
internal infrastructure; they are included to give you an idea of how we
integrate mercurial with our workflows.

Below are snippets about some of the extensions contained here.


smartlog
==========

An extension that adds the 'hg smartlog' command. It prints graph log output
containing only the commits relevant to yourself. Shows your bookmarks, the @
or master bookmark, and any draft commits without bookmarks that you've made
within the past 2 weeks. Any commits in the graph that are skipped are
represented by '...'.

githelp
==========

An extension that adds the 'hg githelp' command. It translates Git commands
into Mercurial commands. Example:

    $ hg githelp -- git rebase origin/master
      hg rebase -d master

    $ hg githelp -- reset --hard HEAD^
      hg strip -r .

So it acts as a useful cheat sheet tool for people moving from Git to Mercurial.


backups
==========

An extension that adds the 'hg backups' command. 'hg backups' prints a list of
recently deleted commits (by reading your .hg/strip-backups directory) and
allows you to recover a commit by doing 'hg backups --recover <commithash>'. It
prints the missing commits in reverse chronological order, and acts as a
pseudo-replacement for Git's reflog.


amend
==========

An extension that adds the 'hg amend --rebase' command. When working with a
stack of commits, it's currently impossible to amend a commit in the middle of
the stack. This extension enables that ability, adds a 'hg amend' command that
invokes 'hg commit --amend', and adds a --rebase flag to 'hg amend --rebase'
that rebase all the children of the commit onto the newly amended version.

If 'hg amend' is run on a commit in the middle of a stack without using
--rebase, the amend succeeds and the old version of the commit is left behind
with a marker bookmark on it 'bookmarkname(preamend)'. The user can then run
'hg amend --fixup' to post-humously rebase the children onto the new version of
the commit.

uncommit
========
Adds a 'hg uncommit' command, which undoes the effect of a local commit. This
allows you to either undo a mistake, or remove files from a commit which
weren't intended for it.

By default it uncommits all the files, and completely hides the changeset.
However, if filenames are specified then it will create a new changeset
excluding those files and leave the files in a dirty state in the working dir.
In all cases, files are left unchanged in the working dir, so other local
changes are unaffected.

Uncommit does work in the middle of a stack of changes (possibly creating a new
head), but cannot be used to undo a merge changeset.

chistedit
==========
An interactive ncurses interface to histedit.

NOTE: This requires python-curses installed and Mercurial's histedit extension
enabled.

This extensions allows you to interactively move around changesets or change
the action to perform while keeping track of possible conflicts.

upgradegeneraldelta
===================

Upgrades manifests to generaldelta in-place, without needing to reclone.

drop
==========
Drops specified changeset from the stack. If the changeset to drop has multiple
children branching off of it, all of them (including their descendants)
will be rebased onto the parent commit.
This command does not support dropping changeset which are a result
 of a merge (have two parent changesets). Public changesets cannot be dropped.

Contributing
============

Patches are welcome as pull requests, though they will be collapsed and rebased
to maintain a linear history.


We (Facebook) have to ask for a "Contributor License Agreement" from someone
who sends in a patch or code that we want to include in the codebase. This is a
legal requirement; a similar situation applies to Apache and other ASF
projects.

If we ask you to fill out a CLA we'll direct you to our
[online CLA page](https://developers.facebook.com/opensource/cla) where you can
complete it easily. We use the same form as the Apache CLA so that friction is
minimal.

License
=======

These extensions are made available under the terms of the GNU General Public
License version 2, or any later version. See the COPYING file that accompanies
this distribution for the full text of the license.
