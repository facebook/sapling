*Warning: This plugin is not yet stabilized. Use to help me identify bugs, but it will be a few weeks before its fully stable.*

*Project status as of 5/27:*  Pretty solid, but a bit slow - can losslessly convert most major scenarios and can handle projects up to several thousand commits. Submodules in Git are not handled. See TODO.txt for full list of things I'm working on.


Hg-Git Mercurial Plugin
=======================

This is the Hg-Git plugin for Mercurial, adding the ability to push and pull to/from a Git server repository from Hg.  This means you can collaborate on Git based projects from Hg, or use a Git server as a collaboration point for a team with developers using both Git and Hg.

The Hg-Git plugin can convert commits/changesets losslessly from one system to another, so you can push via an Hg repository and another Hg client can pull it and their changeset node ids will be identical - Mercurial data does not get lost in translation.  It is intended that Hg users may wish to use this to collaborate even if no Git users are involved in the project, and it may even provide some advantages if you're using Bookmarks (see below).

Dependencies
============

This plugin is implemented entirely in Python - there are no Git binary dependencies, you do not need to have Git installed on your system.  The only dependencies are Mercurial and Dulwich.  The plugin is known to work on Hg versions 1.1 through 1.3 and requires at least Dulwich 0.3.2.

Commands
=========

You can clone a Git repository from Hg by running `hg clone [url]`.  For example, if you were to run `hg clone git://github.com/schacon/munger.git` it would clone the repository down into the directory 'munger.git', then convert it to an Hg repository for you.

    hg clone git://github.com/schacon/hg-git.git

If you want to clone a github repository for later pushing (or any other repository you access via ssh), you need to convert the ssh url to a format with explicit protocol prefix (mind the switch from colon to slash after the host!):

    git clone git@github.com:schacon/hg-git.git
    hg clone git+ssh://git@github.com/schacon/hg-git.git

If you are starting from an existing Hg repository, you have to setup a Git repository somewhere that you have push access to, add it as default path or default-push path in your .hg/hgrc and then run `hg push` from within your project.  For example:

    cd hg-git # (an Hg repository)
    (edit .hg/hgrc and add the target git url in the paths section)
    hg push

This will convert all your Hg data into Git objects and push them up to the Git server.

Now that you have an Hg repository that can push/pull to/from a Git repository, you can fetch updates with `hg pull`.

    $ hg pull

That will pull down any commits that have been pushed to the server in the meantime and give you a new head that you can merge in.

Hg-Git can also be used to convert a Mercurial repository to Git.  As Dulwich doesn't support local repositories yet, the easiest way is to setup up a local SSH server.  Then use the following commands to convert the repository (it assumes your running this in $HOME).

    $ mkdir git-repo; cd git-repo; git init; cd ..
    $ cd hg-repo
    $ hg bookmarks hg
    $ hg push git+ssh://localhost:git-repo

The hg bookmark is necessary to prevent problems as otherwise hg-git pushes to the currently checked out branch confusing Git. This will create a branch named hg in the Git repository. To get the changes in master use the following command (only necessary in the first run, later just use git merge or rebase).

    $ cd git-repo
    $ git checkout -b master hg

To import new changesets into the Git repository just rerun the hg push command and then use git merge or git rebase in your Git repository.


Hg Bookmarks Integration
========================

If you have the bookmarks extension enabled, Hg-Git will use it. It will push your bookmarks up to the Git server as branches and will pull Git branches down and set them up as bookmarks.

This is actually pretty cool, since you can use this extension to transfer your Hg bookmarks via the Git protocol, rather than having to scp them, as the Hg transfer protocol does not currently support transferring bookmarks.

Installing
==========

Clone this repository somewhere and make the 'extensions' section in your `~/.hgrc` file look something like this:

    [extensions]
    hgext.bookmarks =
    hggit = [path-to]/hg-git/hggit

That will enable the Hg-Git extension for you.  The bookmarks section is not compulsory, but it makes some things a bit nicer for you.

Authors
========

* Scott Chacon <schacon@gmail.com> - main development
* Augie Fackler <durin42@gmail.com> - testing and moral support, janitor
* Sverre Rabbelier <sverre@rabbelier.nl> - gexport, mode and i18n stuff and misc fixes
* Abderrahim Kitouni <a.kitouni@gmail.com> - more "mercurialistic" UI
* Kevin Bullock <kbullock@ringworld.org> - proper packaging for PyPI

* Dulwich Developers - most of this code depends on the awesome work they did.

Sponsorship
===========

GitHub let me (Scott) work on this full time for several days, which is why this got done at all.  If you're looking for a free Git host to push your open source Hg projects to, do try us out (http://github.com).
