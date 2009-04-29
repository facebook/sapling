Hg-Git Mercurial Plugin
=======================

This is the Hg-Git plugin for Mercurial, adding the ability to push and pull to/from a Git server repository from Hg.  This means you can collaborate on Git based projects from Hg, or use a Git server as a collaboration point for a team with developers using both Git and Hg.

The Hg-Git plugin can convert commits/changesets losslessly from one system to another, so you can push via an Hg repository and another Hg client can pull it and thier changeset node ids will be identical - Mercurial data does not get lost in translation.  It is intended that Hg users may wish to use this to collaborate even if no Git users are involved in the project, as it may provide some advantages if you're using Bookmarks (see below).

Commands
=========

You can clone a Git repository from Hg by running `hg gclone [url]`.  It will create a directory appended with a '-hg', for example, if you were to run `hg gclone git://github.com/schacon/munger.git` it would clone the repository down into the directory 'munger-hg', then convert it to an Hg repository for you.

	hg gclone git://github.com/schacon/munger.git
	
If you are starting from an existing Hg repository, you have to setup a Git repository somewhere that you have push access to, add it as a Git remote and then run `hg gpush` from within your project.  For example:

	$ cd hg-git # (an Hg repository)
	$ hg gremote add origin git@github.com/schacon/hg-git.git
	$ hg push

This will convert all our Hg data into Git objects and push them up to the Git server.
	
Now that you have an Hg repository that can push/pull to/from a Git repository, you can fetch updates with `hg gfetch`.

	$ hg gfetch
	
That will pull down any commits that have been pushed to the server in the meantime and give you a new head that you can merge in.


Hg Bookmarks Integration
========================

Hg-Git works will use your bookmarks if you have any or have the bookmarks extension enabled.  It will allow you to push your bookmarks up to the Git server as branches and will pull Git branches down and set them up as bookmarks if you want.

This is actually pretty cool, since you can use this extension to transfer your Hg bookmarks via the Git protocol, rather than having to scp them, as the Hg transfer protocol does not currently support transferring bookmarks.

Installing
==========

Clone this repository somewhere and make the 'extensions' section in your `~/.hgrc` file look something like this:

	[extensions]
	hgext.bookmarks =
	hgext.hg-git = [path-to]/hg-git

That will enable the Hg-Git extension for you.  The bookmarks section is not compulsory, but it makes some things a bit nicer for you.
