Basic Use
---------

You can clone a Git repository from Hg by running `hg clone <url> [dest]`.
For example, if you were to run::

 $ hg clone git://github.com/schacon/hg-git.git

Hg-Git would clone the repository and convert it to an Hg repository for
you. There are a number of different protocols that can be used for Git
repositories. Examples of Git repository URLs include::

  https://github.com/schacon/hg-git.git
  http://code.google.com/p/guava-libraries
  ssh://git@github.com:schacon/hg-git.git
  git://github.com/schacon/hg-git.git

For protocols other than git://, it isn't clear whether these should be
interpreted as Mercurial or Git URLs. Thus, to specify that a URL should
use Git, prepend the URL with "git+". For example, an HTTPS URL would
start with "git+https://". Also, note that Git doesn't require the
specification of the protocol for SSH, but Mercurial does.

If you are starting from an existing Hg repository, you have to set up a
Git repository somewhere that you have push access to, add a path entry
for it in your .hg/hgrc file, and then run `hg push [name]` from within
your repository. For example::

 $ cd hg-git # (an Hg repository)
 $ # edit .hg/hgrc and add the target Git URL in the paths section
 $ hg push

This will convert all your Hg data into Git objects and push them to the
Git server.

Pulling new revisions into a repository is the same as from any other
Mercurial source. Within the earlier examples, the following commands are
all equivalent::

 $ hg pull
 $ hg pull default
 $ hg pull git://github.com/schacon/hg-git.git

Git branches are exposed in Hg as bookmarks, while Git remotes are exposed
as Hg local tags.  See `hg help bookmarks` and `hg help tags` for further
information.

Limitations
-----------

- Cloning/pushing/pulling local Git repositories is not supported (due to
  lack of support in Dulwich)
- The `hg incoming` and `hg outgoing` commands are not currently
  supported.