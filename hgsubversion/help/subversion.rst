Basic Use
---------

Converting a Subversion repository to Mercurial with hgsubversion is done by
cloning it. Subversion repositories are specified using the same, regular URL
syntax as Subversion uses. hgsubversion accepts URIs such as the following:

- http://user:sekrit@example.com/repo
- https://user@example.com/repo
- svn://example.com/repo
- svn+ssh://example.com/repo
- file:///tmp/repo

In the case of the two first schemas, HTTP and HTTPS, the repository is first
treated as a Mercurial repository, and a Subversion pull attempted should it
fail. As this can be particularly annoying for repositories that require
authentication, such repositories may also specified using a svn+http or
svn+https schema.

To create a new Mercurial clone, you can use a command such as the following::

 $ hg clone <repository URI> [destination]

Or with a real example::

 $ hg clone http://python-nose.googlecode.com/svn nose-hg

Please note that there are two slightly different ways of cloning repositories:

The most common desire is to have the full history of a repository, including
all its tags and branches. In such cases you should clone from one level above
trunk, as in the example above. This is known as `standard layout`, and works
with repositories that use the conventional `trunk`, `tags` and `branches`
directories. By default, hgsubversion will use this layout whenever it finds any
of these directories at the specified directory on the server.

If you instead want to clone just a single directory or branch, clone the
specific directory path. In the example above, to get *only* trunk, you would
issue :hg:`clone http://python-nose.googlecode.com/svn/trunk nose-trunk`. This
works with any directory with a Subversion repository, and is know as a single
directory clone.

Pulling new revisions into an already-converted repo is the same as from any
other Mercurial source. Within the first example above, the following three
commands are all equivalent::

 $ hg pull
 $ hg pull default
 $ hg pull http://python-nose.googlecode.com/svn

Sometimes, past repository history is of little or no interest, and all that is
wanted is access to current and future history from Mercurial. The --startrev
option with the HEAD argument causes the initial clone to only convert the
latest revision; later pulls will convert all revisions following the first.
Please note that this only works for single-directory clones.

Support for externals
-----------------------------

All ``svn:externals`` properties are serialized into a single
``.hgsvnexternals`` file having the following syntax::

  [.]
   common1 http://path/to/external/svn/repo1
   ...additional svn:externals properties lines...
  [dir2]
   common2 -r123 http://path/to/external/svn/repo2
   ...additional svn:externals properties lines...

A header line in brackets specifies the directory the property applies
to, where '.' indicates the project root directory. The property content
follows the header, with every content line being prefixed by a single
space. Note that the property lines have a format identical to
svn:externals properties as used in Subversion, and do not support the
hgsubversion extended svn+http:// URL format.

Issuing the command :hg:`svn updateexternals` with the ``.hgsvnexternals``
example above would fetch the latest revision of repo1 into the subdirectory
*./common1*, and revision 123 of repo2 into *dir2/common2*.  Note that 
``.hgsvnexternals`` must be tracked by Mercurial before this will work.  If
``.hgsvnexternals`` is created or changed, it
will not be pushed to the related Subversion repository, but its
contents **will** be used to update ``svn:externals`` properties on the
related Subversion repository.

Limitations
-----------

Currently, pushing to Subversion destroys the original changesets and replaces
them with new ones converted from the resulting commits. Due to the intricacies
of Subversion semantics, these converted changesets may differ in subtle ways
from the original Mercurial changests. For example, the commit date almost
always changes. This makes hgsubversion unsuitable for use as a two-way bridge.

When converting from Subversion, hgsubversion does not recognize merge-info, and
does not create merges based on it. Similarly, Mercurial merges cannot be pushed
to Subversion.

Changesets that create tags cannot be pushed to Subversion, as support for
creating Subversion tags has not been implemented, yet.

Standard layout does not work with repositories that use unconventional
layouts. Thus, only a single directory clones can be made of such repositories.

When interacting with Subversion, hgsubversion relies on information about the
previously converted changesets. This information will not be updated if pushing
or pulling converted changesets to or from any other source. To regenerate the
stored metadata, run :hg:`svn rebuildmeta [URI]`. This must also be done if any
converted changesets are ever removed from the repository.

It is not possible to interact with more than one Subversion repository per
Mercurial clone. Please note that this also applies to more than one path within
a repository.

Advanced Configuration
----------------------

The operation of hgsubversion can be customized by the following configuration
settings:

  hgsubversion.authormap
    Path to a file for mapping usernames from  Subversion to Mercurial. For
    example::

      joe = Joe User <joe@example.com>

  hgsubversion.defaulthost
    This option specifies the hostname to append to unmapped Subversion
    usernames. The default is to append the UUID of the Subversion repository
    as a hostname. That is, an author of `bob` may be mapped to
    `bob@0b1d8996-7ded-4192-9199-38e2bec458fb`.

    If this option set to an empty string, the Subversion authors will be used
    with no hostname component.

  hgsubversion.defaultauthors
    Setting this boolean option to true will cause hgsubversion to abort a
    conversion if a revision has an author not listed in the author map.

  hgsubversion.branchmap
    Path to a file for changing branch names during the conversion from
    Subversion to Mercurial.

  hgsubversion.filemap
    Path to a file for filtering files during the conversion. Files may either
    be excluded or included. See the documentation for :hg:`convert` for more
    information on filemaps.

  hgsubversion.username, hgsubversion.password
    Set the username or password for accessing Subversion repositories.

  hgsubversion.stupid
    Setting this boolean option to true will force using a slower method for
    pulling revisions from Subversion. This method is compatible with servers
    using very old versions of Subversion, and hgsubversion falls back to it
    when necessary.

The following options only have an effect on the initial clone of a repository:

  hgsubversion.layout
    Set the layout of the repository. `standard` assumes a normal
    trunk/branches/tags layout. `single` means that the entire repository is
    converted into a single branch. The default, `auto`, causes hgsubversion to
    assume a standard layout if any of trunk, branches, or tags exist within the
    specified directory on the server.

  hgsubversion.startrev
    Convert Subversion revisions starting at the one specified, either an
    integer revision or HEAD; HEAD causes only the latest revision to be pulled.
    The default is to pull everything.

  hgsubversion.tagpaths
    Specifies one or more paths in the Subversion repository that
    contain tags. The default is to only look in `tags`. This option has no
    effect for single-directory clones.

Please note that some of these options may be specified as command line options
as well, and when done so, will override the configuration. If an authormap,
filemap or branchmap is specified, its contents will be read and stored for use
in future pulls.

Finally, the following environment variables can be used for testing a
deployment of hgsubversion:

  HGSUBVERSION_BINDINGS
    By default, hgsubversion will use Subvertpy, but fall back to the SWIG
    bindings. Set this variable to either ``SWIG`` or ``Subvertpy`` (case-
    insensitive) to force that set of bindings.
