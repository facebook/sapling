Segmented Changelog
===================

The core of ``Segmented Changelog`` is an efficient representation for
``Directed Acyclic Graphs``.  This new structure splits the concerns of the
``Changelog`` in 3 parts:

1. graph shape

2. node identifiers (commit hashes)

3. node information (commit messages)


Properties for Segmented Changelog:

* tiny graph shape

* efficient set representation (O(1) space for ``ancestors(master)``) and
  calculations in common cases

* efficient graph calculations, most algorithms range from ``O(1)`` to
  ``O(merges)``, ``ancestor`` and ``common ancestor`` algorithms scale with
  merges

* assumes a main branch that is updated with a pushrebase workflow

* some algorithms (ex. descendants) do not scale well with many visible heads


Segments
--------

Commit hashes are hard to compress. To represent a graph losslessly and
efficiently, integer identities scale much better.

Given a graph, Segmented Changelog will assign (mostly) continuous integer
identifiers to all nodes. The assignment of the integers needs to be a valid
topological sorting of the nodes in the graph.

A segment is a collection of nodes labeled continuously. We can describe
segments by the lowest integer that it contains and the highest integer::

  x:y = [i for i in range(x, y+1)]

Let ``parents(i)`` be the function that returns the list of parents of the
node labelled ``i``.  We describe the parents of a segment as the identifiers
that are parents of nodes in the segment but are not part of the segment
themselves, with the order preserved::

  parents(x:y) = [p for i in range(x, y+1)
                    for p in parents(i)
                    if p not in range(x, y+1)]

As an observation we have the property that the parents of the segment composed
of one note is equal to the parents of the node::

  parents(i:i) == parents(i)

``Segments`` can be stacked to form multiple levels. A higher level segment
merges one or more continuous lower level segments. For example::

  Level 0: 0:5, 6:10, 11:15, 16:20, ...
  Level 1: 0:10, 11:20, ...
  Level 2: 0:20, ...
  Level 3: ...

Segments in the lowest level (0) are called ``Flat Segments``. They
are special. They *and their parents* losslessly encode the shape of
the entire graph. Higher level segments are optional for correctness
but useful for better performance.

``Flat segment`` constraints:

* ``heads(x:y) = [y]``

* ``roots(x:y) = [x]``

* ``((x:y) - x) & merge() = []``

Example::

  3 -- 4 ---\         /- 9 -- 10 -\
            |         |           |
  1 -- 2 -- 5 -- 6 -- 7 -- 8 ---- 11 -- 12

The segments we have are:

* ``1:2``, ``parents(1:2) = []``

* ``3:4``, ``parents(3:4) = []``

* ``5:8``, ``parents(5:8) = [2, 4]``

* ``9:10``, ``parents(9:10) = [7]``

* ``11:12``, ``parents(11:12) = [8, 10]``

We described our graph using the ``5:8`` segment. Note that ``5:8`` and ``5:7``
obey the ``Segment`` definition and can be used in different contexts. We can
describe this as a property:

* For ``x:y`` flat segment, ``x:i`` is a flat segment for ``x <= i <= y`` and
  ``parents(x:y) == parents(x:i)``.

A great property of flat segments is that they compress the original graph to
``O(|merge|)``. The parent function can be derived without any data loss.

High-level Segments.

Constraints:

* ``heads(x:y) = [y]``

Properties:

* Compresses commits across merges.

* Using high level segments we cannot get parents of arbitrary nodes. ``Flat
  Segments`` are required.

Example. Continuing with the graph in the ``Flat Segments`` section, we have
the following high level commits:

* ``1:8``, ``parents(1:8) = []``

* ``9:12``, ``parents(9:12) = [7, 8]``

* ``1:12``, ``parents(1:12) = []``


We can express sets using sets of ``Segments``.

Example:

* ``ancestors(12) = 1:12``

* ``ancestors(11) = 1:8 + 9:10 + 11:11``

* ``ancestors(10) = 1:2 + 3:4 + 5:7 + 9:10``

We can then operate on these sets in more complex ways. For computing common
ancestors we can write::

  ancestor(10, 8)
    = max(::10 & ::8)
    = max((1:7 + 9:10) & 1:8)
    = max(1:7)
    = 7

Implementation Details
----------------------

Assigning sequential integers algorithm
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Any topological order is valid though it may not be efficient. A ``Depth-First
Search`` order assignment yields good results. An optimization that we can do
it is for merges, we first assign the branch with least number of merges.

To assign a number for ``x``, check its parents.

* If all parents have numbers, assign the next available number to ``x``.

* Otherwise, pick the parent with less merges. Assign it recursively.

Groups
~~~~~~

Efficiency of ``Segmented Changelog`` requires efficient representation of
sets.  For example, ``ancestors(master)`` is ideally ``0:x``, not ``0:i +
j:x``. To ensure that, ``Segmented Changelog`` reserves a large integer space
for master and tries to avoid assigning master + 1 to non-master nodes. This is
achieved by ``Groups``.

In our ``Changelog`` use case, we have the ``Master`` group and the
``NonMaster`` group. It is generally assumed that the ``Master`` group will
have a lot of commits (and we mostly care about them). ``Master`` commits are
assigned starting from ``0``.  ``NonMaster`` commits are assigned from ``2^56``
onwards.  Local commits are stored in the ``NonMaster`` section. This has the
purpose of preserving efficient ``Segment`` construction for the ``Master``
section as new commits get added of top of this section. This allows the
``Master`` section to have segments with maximum length.

Core Components
~~~~~~~~~~~~~~~

We described 3 concepts in the beginning:

1. graph shape, we may also call ``IdDag``.

2. node identifiers, we may also call ``IdMap``.

3. node information, we may also call ``HgCommits``.

The ``IdDag`` describes all graph algorithms and all operations leverage
segments.  The ``IdMap`` is a bidirectional map from ``Segmented Changelog Id``
to node identifier, in the case of the changelog the ``Sha1`` hash of the
commit.  ``HgCommits`` is a simple key value store from ``Sha1`` commit hash to
commit message.

For the client ``IdDag``, ``IdMap`` and ``HgCommits`` are stored using
``IndexedLog``.

Server implementation
~~~~~~~~~~~~~~~~~~~~~

For the server, ``IdDag`` is stored fully in process for all serving instances.
At startup, a ``IdDag`` is downloaded from the ``Blobstore``, then it is
updated locally as commits come in. Protocols do not depend on "internal"
``Segmented Changelog Ids``, instead they express shape relative to common node
identifiers (commit hashes).

The ``IdMap`` is stored in a ``SQL`` store. For the servers, the ``IdMap``
additionally stores a version. This is done to facilitate regenerating the
whole ``IdMap`` while continuing to serve requests. ``IdMap`` regeneration may
be performed when better algorithms are developed or bugs are uncovered.

``HgCommits`` have their own ``SQL`` storage. For the purpose of ``Segmented
Changelog`` we only care that we can query the respective commit identifier that
is stored in the ``IdMap``.

To construct the ``Segmented Changelog`` from a repository we use the
``Segmented Changelog Seeder``.  The ``Seeder`` will construct a new ``IdMap``
version and an initial ``IdDag``.

We mentioned that at startup an ``IdDag`` has to be loaded from the
``Blobstore`` so we need a process that updates the ``IdMap`` and the
``IdDag``. This process is the ``Segmented Changelog Tailer``. It periodically
checks for new commits and incrementally adds entries to the ``IdMap`` and
reconstructs the ``IdDag``.

The client communicates with the server using ``EdenApi`` endpoints:

* ``/{repo}/commit/revlog_data``: commit content by ``Sha1`` hash in the format
  that verifies the ``Sha1`` hash.

* ``/{repo}/commit/location_to_hash``: used to translate compressed commit info
  (graph location) to hashes.

* ``/{repo}/commit/hash_to_location``:used to translate hashes to compressed
  commit info (graph location).

* ``/{repo}/clone``: downloads ``SegmentedChangelog`` repository data.

* ``/{repo}/full_idmap_clone``: downloads ``SegmentedChangelog`` repository
  data along with all commit hashes in the commit graph.

