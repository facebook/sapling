Basic Use
---------

Converting a Subversion repository to Mercurial with hgsubversion is done by
cloning it. Subversion repositories are specified using the same, regular URL
syntax that Subversion uses. hgsubversion accepts URIs such as the following::

  http://user:sekrit@example.com/repo
  https://user@example.com/repo
  svn://example.com/repo
  svn+ssh://example.com/repo
  file:///tmp/repo

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
with repositories that use the conventional ``trunk``, ``tags`` and ``branches``
directories. By default, hgsubversion will use this layout whenever it finds any
of these directories at the specified directory on the server.  Standard layout
also supports alternate names for the ``branches`` directory and multiple tags
locations.  Finally, Standard Layout supports selecting a subdirectory relative
to ``trunk``, and each branch and tag dir.  This is useful if you have a single
``trunk``, ``branches``, and ``tags`` with several projects inside, and you wish
to import only a single project.

If you instead want to clone just a single directory or branch, clone the
specific directory path. In the example above, to get *only* trunk, you would
issue ``hg clone http://python-nose.googlecode.com/svn/trunk nose-trunk``. This
works with any directory with a Subversion repository, and is known as a single
directory clone. Normally, converted changesets will be marked as belonging to
the ``default`` branch, but this can be changed by using the ``-b/--branch``
option. To force single directory clone, use hgsubversion.layout option (see
below for detailed help) ::

 $ hg clone --layout single svn+http://python-nose.googlecode.com/svn nose-hg

Finally, if you want to clone two or more directores as separate
branches, use the custom layout.  See the documentation below for the
``hgsubversionbranch.*`` configuration for detailed help.

Pulling new revisions into an already-converted repository is the same
as from any other Mercurial source. Within the first example above,
the following three commands are all equivalent::

 $ hg pull
 $ hg pull default
 $ hg pull http://python-nose.googlecode.com/svn

Sometimes, past repository history is of little or no interest, and
all one wants is to start from today and work forward. Using
``--startrev HEAD`` causes the initial clone to only convert the
latest revision; later pulls will convert all subsequent
revisions. Please note that this only works for single-directory
clones::

 $ hg clone --startrev HEAD http://python-nose.googlecode.com/svn/trunk nose-hg

Finding and displaying Subversion revisions
-------------------------------------------

For revealing the relationship between Mercurial changesets and
Subversion revisions, hgsubversion provides three template keywords:

  :svnrev: Expanded to the original Subversion revision number.
  :svnpath: The path within the repository that the changeset represents.
  :svnuuid: The Universally Unique Identifier of the Subversion repository.

An example::

  $ hg log --template='{rev}:{node|short} {author|user}\nsvn: {svnrev}\n'

For finding changesets from Subversion, hgsubversion extends revsets
to provide two new selectors:

  :fromsvn: Select changesets that originate from Subversion. Takes no
    arguments.
  :svnrev: Select changesets that originate in a specific Subversion
    revision. Takes a revision argument.

For example::

  $ hg log -r 'fromsvn()'
  $ hg log -r 'svnrev(500)'

See ``hg help revsets`` for details.

Support for externals
---------------------

Subversion externals conversion is implemented for standard layouts.

Using .hgsvnexternals (default mode)
====================================

.hgsvnexternals has been implemented before Mercurial supported proper
subrepositories. Externals as subrepositories should now be preferred
as they offer almost all .hgsvnexternals features with the benefit of
a better integration with Mercurial commands.

``svn:externals`` properties are serialized into a single
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

Issuing the command ``hg svn updateexternals`` with the ``.hgsvnexternals``
example above would fetch the latest revision of `repo1` into the subdirectory
`./common1`, and revision 123 of `repo2` into `dir2/common2`. Note that
``.hgsvnexternals`` must be tracked by Mercurial before this will work. If
``.hgsvnexternals`` is created or changed, it
will not be pushed to the related Subversion repository, but its
contents **will** be used to update ``svn:externals`` properties on the
related Subversion repository.

Alternatively, one can use the ``hgsubversion.externals`` in hgrc to
specify ``subrepos`` as the externals mode. In this mode, ``.hgsub``
and ``.hgsubstate`` files will be used instead of
``.hgsvnexternals``. This feature requires Mercurial 1.7.1 or later.


Using Subrepositories
=====================

Set:

  [hgsubversion]
  externals = subrepos

to enable this mode.

``svn:externals`` properties are serialized into the subrepositories
metadata files, ``.hgsub`` and ``.hgsubstate``. The following
``svn:externals`` entry:

  -r23 ^/externals/project1 deps/project1

set on the "subdir" directory becomes:

    (.hgsub)
    subdir/deps/project1 = [hgsubversion] subdir:-r{REV} ^/externals/project1 deps/project1

    (.hgsubstate)
    23 subdir/deps/project1

At this point everything works like a regular svn subrepository. The
right part of the .hgsub entry reads like:

    TARGETDIR:REWRITTEN_EXTERNAL_DEFINITION

where REWRITTEN_EXTERNAL_DEFINITION is like the original definition
with the revision identifier replaced with {REV}.

This mode has the following limitations:

* Require Mercurial >= 1.7.1 to work correctly on all platforms.

* "hgsubversion" subrepositories require hgsubversion extension to be
  available. To operate transparently on ``svn:externals`` we have to
  stay as close as possible to their original property
  format. Besides, relative externals require a parent subversion
  repository to be resolved while stock Mercurial only supports
  absolute subversion paths.

* Leading or trailing whitespaces in the external definitions are lost

* Leading or trailing whitespaces in the target directory are lost

* The external definition should not contain {REV}

* Unversioned definitions are pulled but the behaviour upon
  update/merge is not clearly defined. We tried to preserve the
  .hgsubstate as "HEAD" but the subrepository will probably not be
  updated when the hg repository is updated. Given subrepositories
  were designed not to support unversioned dependencies, this is
  unlikely to be fixed.

* .hgsub and .hgsubstate are currently overwritten and
  non-[hgsubversion] subrepos entries are lost. This could be fixed by
  editing these files more carefully.

Limitations
-----------

Currently, pushing to Subversion destroys the original changesets and replaces
them with new ones converted from the resulting commits. Due to the intricacies
of Subversion semantics, these converted changesets may differ in subtle ways
from the original Mercurial changesets. For example, the commit date almost
always changes. This makes hgsubversion unsuitable for use as a two-way bridge.

When converting from Subversion, hgsubversion does not recognize merge-info, and
does not create merges based on it. Similarly, Mercurial merges cannot be pushed
to Subversion.

Changesets that create tags cannot be pushed to Subversion, as support for
creating Subversion tags has not yet been implemented.

Standard layout does not work with repositories that use unconventional
layouts. Thus, only single directory clones can be made of such repositories.

When interacting with Subversion, hgsubversion relies on information about the
previously converted changesets. This information will not be updated if pushing
or pulling converted changesets to or from any other source. To regenerate the
stored metadata, run ``hg svn rebuildmeta [URI]``. This must also be done if any
converted changesets are ever removed from the repository.

Under certain circumstances a long-running conversion can leak substantial
amounts of memory, on the order of 100MB per 1000 converted revisions. The
leaks appear to be persistent and unavoidable using the SWIG bindings. When
using the new experimental Subvertpy bindings, leaks have only been observed
accessing FSFS repositories over the file protocol.

Should the initial clone fail with an error, Mercurial will delete the entire
repository, including any revisions successfully converted. This can be
particularly undesirable for long-running clones. In these cases, we suggest
using the ``-r/--rev`` option to only clone a few revisions initially. After
that, an ``hg pull`` in the cloned repository will be perfectly safe.

It is not possible to interact with more than one Subversion repository per
Mercurial clone. Please note that this also applies to more than one path within
the same Subversion repository.

Mercurial does not track directories, and as a result, any empty directories
in Subversion cannot be represented in the resulting Mercurial repository.

Externals support requires that the ``svn`` command line utility is available.
In addition, externals support has been disabled for single directory clones,
due to known bugs.

Advanced Configuration
----------------------

The operation of hgsubversion can be customized by the following configuration
settings:

  ``hgsubversion.authormap``

    Path to a file for mapping usernames from Subversion to Mercurial. For
    example::

      joe = Joe User <joe@example.com>

    Some Subversion conversion tools create revisions without
    specifying an author. Such author names are mapped to ``(no
    author)``, similar to how ``svn log`` will display them.

  ``hgsubversion.defaulthost``

    This option specifies the hostname to append to unmapped Subversion
    usernames. The default is to append the UUID of the Subversion repository
    as a hostname. That is, an author of ``bob`` may be mapped to
    ``bob@0b1d8996-7ded-4192-9199-38e2bec458fb``.

    If this option set to an empty string, the Subversion authors will be used
    with no hostname component.

  ``hgsubversion.defaultmessage``

    This option selects what to substitute for an empty log
    message. The default is to substitute three dots, or ``...``.

  ``hgsubversion.defaultauthors``

    Setting this boolean option to false will cause hgsubversion to abort a
    conversion if a revision has an author not listed in the author map.

  ``hgsubversion.branch``

    Mark converted changesets as belonging to this branch or, if unspecified,
    ``default``. Please note that this option is not supported for standard
    layout clones.

  ``hgsubversion.branchmap``

    Path to a file for changing branch names during the conversion from
    Subversion to Mercurial.

  ``hgsubversion.branchdir``

    Specifies the subdirectory to look for branches under.  The
    default is ``branches``.  This option has no effect for
    single-directory clones.

  ``hgsubversion.infix``

    Specifies a path to strip between relative to the trunk/branch/tag
    root as the mercurial root.  This can be used to import a single
    sub-project when you have several sub-projects under a single
    trunk/branches/tags layout in subversion.

  ``hgsubversion.filemap``

    Path to a file for filtering files during the conversion. Files may either
    be included or excluded. See the documentation for ``hg convert`` for more
    information on filemaps.

  ``hgsubversion.filestoresize``

    Maximum amount of temporary edited files data to be kept in memory,
    in megabytes. The replay and stupid mode pull data by retrieving
    delta information from the subversion repository and applying it on
    known files data. Since the order of file edits is driven by the
    subversion delta information order, edited files cannot be committed
    immediately and are kept until all of them have been processed for
    each changeset. ``filestoresize`` defines the maximum amount of
    files data to be kept in memory before falling back to storing them
    in a temporary directory. This setting is important with
    repositories containing many files or large ones as both the
    application of deltas and Mercurial commit process require the whole
    file data to be available in memory. By limiting the amount of
    temporary data kept in memory, larger files can be retrieved, at the
    price of slower disk operations. Set it to a negative value to
    disable the fallback behaviour and keep everything in memory.
    Default to 200.

  ``hgsubversion.username``, ``hgsubversion.password``

    Set the username or password for accessing Subversion repositories.

  ``hgsubversion.password_stores``

    List of methods to use for storing passwords (similar to the option of the
    same name in the subversion configuration files). Default is
    ``gnome_keyring,keychain,kwallet,windows``. Password stores can be disabled
    completely by setting this to an empty value.

    .. NOTE::

        Password stores are only supported with the SWIG bindings.

  ``hgsubversion.stupid``
    Setting this boolean option to true will force using a slower method for
    pulling revisions from Subversion. This method is compatible with servers
    using very old versions of Subversion, and hgsubversion falls back to it
    when necessary.

  ``hgsubversion.externals``
    Set to ``subrepos`` to switch to subrepos-based externals support
    (requires Mercurial 1.7.1 or later.) Default is ``svnexternals``,
    which uses a custom hgsubversion-specific format and works on
    older versions of Mercurial. Use ``ignore`` to avoid converting externals.

The following options only have an effect on the initial clone of a repository:

  ``hgsubversion.layout``

    Set the layout of the repository. ``standard`` assumes a normal
    trunk/branches/tags layout. ``single`` means that the entire
    repository is converted into a single branch. The default,
    ``auto``, causes hgsubversion to assume a standard layout if any
    of trunk, branches, or tags exist within the specified directory
    on the server.  ``custom`` causes hgsubversion to read the
    ``hgsubversionbranch`` config section to determine the repository
    layout.

  ``hgsubversion.startrev``

    Convert Subversion revisions starting at the one specified, either an
    integer revision or ``HEAD``; ``HEAD`` causes only the latest revision to be
    pulled. The default is to pull everything.

  ``hgsubversion.tagpaths``

    Specifies one or more paths in the Subversion repository that
    contain tags. The default is to only look in ``tags``. This option has no
    effect for single-directory clones.

  ``hgsubversion.unsafeskip``

    A space or comma separated list of Subversion revision numbers to
    skip over when pulling or cloning.  This can be useful for
    troublesome commits, such as someone accidentally deleting trunk
    and then restoring it.  (In delete-and-restore cases, you may also
    need to clone or pull in multiple steps, to help hgsubversion
    track history correctly.)

    NOTE: this option is dangerous.  Careless use can make it
    impossible to pull later Subversion revisions cleanly, e.g. if the
    content of a file depends on changes made in a skipped rev.
    Skipping a rev may also prevent future invocations of ``hg svn
    verify`` from succeeding (if the contents of the Mercurial repo
    become out of step with the contents of the Subversion repo).  If
    you use this option, be sure to carefully check the result of a
    pull afterwards.

    ``hgsubversionbranch.*``

    Use this config section with the custom layout to specify a cusomt
    mapping of subversion path to Mercurial branch.  This is useful if
    your layout is substantially different from the standard
    trunk/branches/tags layout and/or you are only interested in a few
    branches.

    Example config that pulls in trunk as the default branch,
    personal/alice as the alice branch, and releases/2.0/2.7 as
    release-2.7::

        [hgsubversionbranch]
            default = trunk
            alice = personal/alice
            release-2.7 = releases/2.0/2.7

    Note that it is an error to specify more than one branch for a
    given path, or to sepecify nested paths (e.g. releases/2.0 and
    releases/2.0/2.7)

Please note that some of these options may be specified as command line options
as well, and when done so, will override the configuration. If an authormap,
filemap or branchmap is specified, its contents will be read and stored for use
in future pulls.

Finally, the following environment variables can be used for testing a
deployment of hgsubversion:

  ``HGSUBVERSION_BINDINGS``

    By default, hgsubversion will use Subvertpy, but fall back to the SWIG
    bindings. Set this variable to either ``SWIG`` or ``Subvertpy`` (case-
    insensitive) to force that set of bindings.
