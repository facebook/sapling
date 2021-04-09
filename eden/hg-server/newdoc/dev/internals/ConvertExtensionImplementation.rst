How the Convert extension works
===============================

This page documents the implementation of the Convert extension as of f29b674cc221.

If you are a user, you should read the Convert extension page instead. If you still intend to read this page, you should read Convert extension first, so that you have a full understanding of what the implementation is implementing.

Inputs
------

Every VCS that the Convert extension supports as input is represented by a subclass of the abstract class ``common.converter_source``. The repository to convert is represented by an instance of a source. For example, when converting from Subversion, the input repository is represented by an instance of ``convert.subversion.svn_source``, which is a subclass of ``converter_source``.

The ``converter_source`` class takes three arguments:

* ``ui``

* ``path`` (default: ``None``)

* ``rev`` (default: ``None``)

Additionally, it has a property named ``encoding``, which is ``'utf-8'`` by default.

If a ``converter_source`` subclass finds no valid repository at the given ``path``, it raises the exception ``common.NoRepo``. The subclass may also raise ``NoRepo`` if some library that it depends on isn't available.

The Subversion source (``convert.subversion.svn_source``) uses “url”, not “path”, as the name of its second argument, because Subversion works with URLs rather than paths. However, it does support local file paths; it uses the ``geturl`` function in the same module to silently upgrade a path to a URL. Additionally, the ``url`` argument is not optional.

Abstract Methods
~~~~~~~~~~~~~~~~

before and after
::::::::::::::::

Subclasses can implement these to perform any necessary preparation and clean-up.

getheads
::::::::

Returns the revision identifiers that exist in the source repository. For most Subversion repositories, the heads are the revision numbers of the latest revision on the trunk and the latest revision on every branch.

getchanges
::::::::::

Takes a revision identifier as its only argument, and returns either a tuple of collections identifying all the files affected by that commit, or ``common.SKIPREV``.

The former collection is a sorted list of tuples. The former object in each tuple is the filename; the latter object is the identifier of that file as of that revision.

Many sources will put the same identifier into every tuple—specifically, the identifier for the requested revision. The Subversion and Mercurial sources both do this. However, some VCSs (such as Git) assign a different identifier to every file-revision intersection; the Git source returns these identifiers.

The latter collection is a mapping of filenames to other filenames. This collection expresses copies: the key filename is one from the list of tuples, and is the destination of the copy, whereas the value filename is the file that was copied (the source of the copy).

If a file was not copied in the commit in question, it is not included in the mapping. This means that the mapping can be, and usually is, empty.

Instead of the pair of collections, the ``getchanges`` method can return ``common.SKIPREV``, if the source wants to indicate that the requested commit should not be converted.

gettags
:::::::

Returns a dictionary in which each key is a tag name and each value is a revision identifier for the commit that was tagged.

Outputs
-------

Output classes are called *sinks*. Every VCS that the Convert extension supports as output is represented by a subclass of the abstract class ``common.converter_sink``.

The ``converter_sink`` class takes two arguments:

* ``ui``

* ``path``

Unlike the ``converter_source`` class, the ``path`` argument here is not optional.

Additionally, it has a property named ``created_files``, which is a list that holds the paths to files that the sink has created, so that the sink can unlink them if the conversion fails. The abstract class does not handle this clean-up; it leaves it to the subclasses.

Abstract methods
~~~~~~~~~~~~~~~~

before and after
::::::::::::::::

Subclasses can implement these to perform any necessary preparation and clean-up.

getheads
::::::::

Returns the revision identifiers that already exist in the destination repository.

authorfile
::::::::::

A subclass can implement this to return the path to a file within the repository where an authormap file may be found.

revmapfile
::::::::::

Every subclass must implement this to return the path to a file within the repository where the converter should store its revision map (described below).

setbranch
:::::::::

Sets the branch that future commits will be on (like the ``hg branch`` command). Takes two arguments: the branch name, and a list of parent branches.

Each parent branch in the list is described by a tuple. The former element is the revision identifier in the destination repository for the parent commit; the latter element is the name of the parent branch.

putcommit
:::::::::

This is the method that enters a commit into the destination repository. Every subclass must implement it.

It takes five arguments:

* ``files``: A list of file references (in the format returned by ``converter_source.getchanges``)

* ``copies``: A mapping containing copy information (in the format returned by ``converter_source.getchanges``)

* ``parents``: A list of revision identifiers from the destination repository, which name the parent commit(s) for the new commit

* ``commit``: A commit object (instance of ``common.commit``)

* ``source``: The source object for the source repository

According to the docstring for ``converter_sink.putcommit``, the source object in the fifth argument is only guaranteed to have ``getfile`` and ``getmode`` methods. In practice, it is always a ``converter_source``, so it will implement all of that class's required methods (although you shouldn't need any others).

The convert command
-------------------

The command is implemented in the ``convert.convcmd`` sub-module. Only the most basic requirements for a Mercurial extension command are in ``convert.__init__``; the ``convert`` function there tail-calls ``convert.convcmd.convert``.

The ``convert`` function calls two subroutines in the same module, ``convertsink`` and ``convertsource``, to obtain sink and source instances for the destination and source repositories. These functions iterate the mappings of VCS names to sink/source classes, trying each class in turn on the specified destination and source repositories.

The final step in the function is to create an instance of ``convcmd.converter``, which is the class that actually performs the conversion.

Anatomy of convcmd.converter
----------------------------

The class takes five arguments, all required:

* ``ui``

* ``source``: An instance of ``converter_source``

* ``dest``: An instance of ``converter_sink``

* ``revmapfile``: The path to the revision map file, which the converter uses to resume conversions

* ``opts``

Additionally, it has five properties:

* ``commitcache``: A dictionary mapping revision identifiers (from the source repository) to ``common.commit`` objects

* ``authors``: A dictionary mapping author names from the source repository to author names in the destination repository, using the union of the destination's author-map file and the author-map file specified on the command-line

* ``authorfile``: The path to the destination repository's author-map file (``self.dest.authorfile()``)

* ``map``: The revision map (described below)

* ``splicemap``: The splice map (described below)

The main method of the class is ``convert``, which the top-level ``convcmd.convert`` function calls to do the work.

The revision map
~~~~~~~~~~~~~~~~

The revision map associates each commit in the source repository with a commit in the destination repository. This is the converter's record of which commits it has already copied. If the user runs the converter again, it reads the revision map back in, and uses it to resume the conversion rather than start it over from the beginning.

A source revision identifier's matching value is usually a destination revision identifier, but may instead be ``common.SKIPREV``. This indicates not that the commit has already been converted, but that the converter should skip it.

What the preceding paragraphs boil down to is that the converter will not copy a commit if its source revision identifier is in the revision map at all, on the assumption that either a previous run copied it or the source didn't want the converter to copy it.

A revision map is an instance of ``common.mapfile``, a subclass of ``dict`` that reads its pairs in from a file, and updates that file whenever another object adds, changes, or removes a pair. The converter uses a file inside the destination repository, whose pathname it obtains from the sink's ``revmapfile`` method.

In the file format, ``common.SKIPREV`` is represented by the word “SKIP” in all uppercase letters. The Convert extension implements this by defining ``common.SKIPREV`` to that string.

The splice map
~~~~~~~~~~~~~~

The splice map enables the user to revise history, giving a commit one or two different parents from the parent(s) it has in the source repository. By adding lines to the splice map, the user can splice one series of commits in between two other commits, remove commits from the history (by connecting their antecedent and descendant directly together), or forge a merge (by adding a second parent to a commit).

The file format of the splice map is simple: each splice is a line, with two or three revision identifiers separated by spaces. The first one is from the source repository, and names the commit whose parents are to be edited. The second and optional third are from either the source or destination repository, and name the commits that will be the new parents.

Like the revision map, the splice map is an instance of ``common.mapfile``. Unlike the revision map, the converter does not change the contents of the splice map.

Commit ordering
~~~~~~~~~~~~~~~

Order is significant, as revision identifiers in Mercurial are dependent on the order of the commits. (Mercurial defines a revision identifier as the hash of a number of pieces of data from the commit, one of which is the revision number of the commit's parent.)

By default, the Convert extension copies commits in topological order, aka ancestral order. As you might guess from the latter name, this means only that a commit is guaranteed to come before a commit that depends on it.

With the ``--datesort`` option, the Convert extension instead copies commits in the order in which they were originally committed in the source repository. As long as humans are not capable of time travel and the repository itself has not been tampered with, this chronological sort is also a valid topological sort.

Both sorts are performed by the ``converter.toposort`` method.

The conversion process
----------------------

Conversion truly begins in the ``converter.convert`` method, although most of the real work is still done in other methods (not to mention other classes).

First, the converter must determine the commits to copy. It starts by getting the list of heads from the source repository (using ``converter_source.getheads``); then, it uses ``converter.walktree`` to find all the ancestors of those heads.

``walktree`` returns an object that maps each commit to a list of its parents. All the commits in this mapping are those that have not yet been copied to the destination repository; when it encounters a commit that is in the converter's revision map, it skips that commit without putting it into the mapping.

The ``convert`` method calls the ``toposort`` method with this mapping to put them in order (see the section describing commit ordering). ``toposort`` takes the mapping, iterates the keys (which are revision identifiers from the source repository), builds a new list containing them in the sorted order, and returns that list.

Now it's time to begin copying commits. For every commit identified in the list, the ``convert`` method calls ``converter.copy`` with that revision identifier (from the source repository).

The ``copy`` method starts out by calling ``self.source.getchanges``, passing the revision identifier. It checks for two unusual cases:

* ``getchanges`` returned ``common.SKIPREV``: The ``copy`` method adds the revision identifier to the revision map with ``SKIPREV`` as the value, then returns.

* ``getchanges`` returned a different string: It's another revision identifier from the source repository. The ``copy`` method looks up that identifier in the revision map, then adds the identifier it started with as another key for the same value, so that the identifier ``copy`` started with and the identifier ``getchanges`` returned are both mapped to the same identifier. Finally, ``copy`` returns.

If neither of those cases is true, then ``getchanges`` returned the usual pair of collections (described above), and the ``copy`` method proceeds.

Next, it assembles a list of parent branches, then calls ``self.dest.setbranch`` with the branch name and that list. (See the description of that method, which covers what the list contains.)

The ``copy`` method then looks up the revision identifier from the source repository in the splice map. If the look-up succeeds, it looks up the parents named by the splice map in the revision map; otherwise, it uses the parent revision identifiers from the list of parent branches (which are already revision identifiers from the destination repository).

Now, finally, the ``copy`` method calls the sink's ``putcommit`` method. It passes the list of files, the mapping of copies, the list of parent revision identifiers from the destination repository, the commit object, and the source object, and receives a revision identifier from the destination repository for the newly-entered commit.

The last things that the ``copy`` method does before returning are to tell the source it converted the commit (by calling ``self.source.converted`` with both revision identifiers), and to enter the revision identifiers into the revision map.

We arrive back in the ``convert`` method, at the end of the loop.

Having finished copying commits, the ``convert`` method updates the tags in the destination repository. It asks the source for its dictionary of tags, then filters each tagged commit's revision identifier through the revision map (skipping any commit that is not in the map or is mapped to ``SKIPREV``).

It then passes this list of destination revision identifiers to the sink's ``puttags`` method, which creates all those tags in the destination repository in a single commit and returns the identifier for that commit. The ``convert`` method then maps the source revision identifier for the last converted commit to the destination revision identifier for the tags-update commit, “so we don't end up with extra tag heads”.

The very last step is to write out the author map into a file in the destination repository (whose pathname the converter gets from the sink's ``authorfile`` method). This file includes any author mapping that the user specified in a custom author-map file when running the convert command.

After this, the converter performs clean-up. It calls the ``after`` methods of the sink, then the source, and closes its revision-map file.

The conversion is done.

