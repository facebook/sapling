Data structures
---------------

**Nodeids** are unique ids that represent the contents of a file *and* its position in the project history.

A **revlog**, for example ``.hg/data/somefile.d``, is the most important data structure and represents all versions of a file. See Revlog_.

A **manifest** describes the state of a project by listing each file and its nodeid to specify which version.

A **changeset** lists all files changed in a checkin along with a change description and metadata like user and date.

Putting it all together
-----------------------

We now have enough information to see how this all works together. To look up a given revision of a file:

* look up the changeset in the changeset index

* reconstruct the changeset data from the revlog

* look up the manifest nodeid from the changeset in the manifest index

* reconstruct the manifest for that revision

* find the nodeid for the file in that revision

* look up the revision of that file in the file's index

* reconstruct the file revision from the revlog

If we want to go the other way and find the changeset associated with a given file revision, we follow the linkrev.

::

      .  .--------linkrev-------------.
         v                            |
      .---------.    .--------.    .--------.
      |changeset| .->|manifest| .->|file    |---.
      |index    | |  |index   | |  |index   |   |--.
      `---------' |  `--------' |  `--------'   |  |
          |       |      |      |     | `-------'  |
          V       |      V      |     V    `-------'
      .---------. |  .--------. |  .---------.
      |changeset|-'  |manifest|-'  |file     |
      |data     |    |data    |    |revision |
      `---------'    `--------'    `---------'

Tracking Working Directory State
--------------------------------

The other piece of Mercurial is the working directory. Mercurial tracks various information about the working directory. See DirState_.

