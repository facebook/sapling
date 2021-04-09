This attempts to describe the project's architecture in terms of modules. In general, no layer should know the details of any layer above it, and no layer should abuse the interface of layers below it. See Design_ for an overview of how things work.

Interface layer
~~~~~~~~~~~~~~~

This is the topmost layer and is the part most directly exposed to the user.

* commands.py - implementation of the command line interface

    This contains most of the code that deals with converting commands into simple repository operations as well as the stdio push/pull interface

* hgweb.py - implementation of the web interface

    This contains all the web and templating logic as well as the web based push/pull interface

Repository layer
~~~~~~~~~~~~~~~~

This layer contains all the objects that implement the core primitives of the SCM:

* commit

* checkout/update/merge

* push/pull

* add/remove/copy

* verify

It also contains the proxy objects for remote repositories:

* remoterepository

* sshrepository

* httprepository

* httpsrepository

* statichttprepository

Finally, it contains the objects from which the repository is constructed:

* filelog - history of individual files

* manifest - contents of project revisions

* changeset - descriptions of project changes

* dirstate - tracking of working directory contents

Storage layer
~~~~~~~~~~~~~

This contains the basis for version storage, revlog, which provides these facilities:

* indexing of revision graph of an object

* compressed delta storage and retrieval

* basic graph algorithms common to multiple object types

* packing and unpacking of delta groups

UI layer
~~~~~~~~

This provides generic methods for communicating with the user and managing configuration info.

Currently this is provided by ui.py which implements text-based methods, but future GUI systems should superclass this.

Utility layer
~~~~~~~~~~~~~

This includes generic functionality and platform abstraction

* calculating diffs (bdiff)

* applying diffs (mpatch)

* manipulating diffs (mdiff)

* OS-related utilities and miscellaneous functions (util)

* internal version numbering (version)

* module demand-loading (demandload)

* locking (lock)

* transactions and rollback (transaction)

* manipulating node strings (node)

.. _Design: Design

