Requires File
=============

The **requires file** is a file inside the repository which is used to describe the general format and layout of the repository. This provides a basic mechanism for introducing new repository formats. It is only written on repository creation, for instance during init or clone.

Details
-------

Starting with Mercurial version 0.9.2 there is a file ``.hg/requires`` which specifies the capabilities needed by a client to access this repository. It is a text file, where each line contains the name of a capability and optionally (separated by a ``=`` character from the name) a list of parameters, each separated by a comma (``,``).

Currently there are only capabilities which don't need parameters.

The requires file only describes the format of the persistent representation of a repository on disk or backup media. Which is completely unrelated to how this information is transferred over the wire (for example, when pushing and pulling).

Older Mercurial versions
------------------------

If an older Mercurial version tries to access a repository that was created by a newer Mercurial version, an error message like

::

   abort: requirement 'dotencode' not supported!

may be displayed, which means the Mercurial version used to access that repository doesn't know how to interpret it, because accessing it would require knowledge about the 'dotencode' capability.

If such an error message appears, a newer Mercurial version must be used to access the repository or the repository must be converted to an older format understood by that version (by using '``hg clone --pull``').

The format configuration option may be used to instruct Mercurial to create older repository formats. For example, to convert a 'dotencode' repository into the previous format, the command

::

   hg --config format.dotencode=0 clone --pull repoA repoB

can be used, which of course requires a Mercurial version that supports the 'dotencode' capability.

Known requirements
------------------

===============  ========================  =================================================================================================================================
Requirement      Introduced with version   Description   
---------------  ------------------------  ---------------------------------------------------------------------------------------------------------------------------------
``revlogv1``     0.9                       RevlogNG_ is used   
``store``        0.9.2                     The directory ``.hg/store`` contains the subdirectories ``data``
``fncache``      1.1                       store files are named to work around Windows limitations
``shared``       1.3                       shared store support   
``dotencode``    1.7                       Leading '.' (period) or ' ' (space) in store filenames are encoded (http://selenic.com/repo/hg/rev/34d8247a4595|34d8247a4595)   
``parentdelta``  1.7 (experimental)        Use parentdelta for new revlogs (still experimental, subject to on-disk format change   
===============  ========================  =================================================================================================================================

.. _RevlogNG: RevlogNG
