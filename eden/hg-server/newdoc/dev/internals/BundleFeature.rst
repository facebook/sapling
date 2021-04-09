Bundle File
===========

All about handling unknown feature in bundle.

Mercurial can store history information into "bundle file" for sharing or backup purpose. The format and feature of these bundle evolve over time in a way that can make new client produce bundle that and older client cannot read.

Known Feature
-------------

===========  ========================  ===============================================
Feature      Introduced with version   Description
-----------  ------------------------  -----------------------------------------------
HG10         0.7                       the old historical format for mercurial bundles
HG20         3.5                       A more extensible bundle format
Compression  3.6                       compression for HG20 bundle
===========  ========================  ===============================================


Producing Compatible Bundle
---------------------------

You can convert bundle from one format to another using:

::

       cd yourepository/
       hg bundle --rev 'bundle()' --base 'parents(roots(bundle()))' -R EXISTINGBUNDLE NEWBUNDLE --type NEWFORMAT

Here is a list of useful type of bundle as in latest Mercurial release. See ``hg help bundle`` for details about the ``--type`` argument.

========== ==================
Type       feature
---------- ------------------
v1         HG10
none-v2    HG20
v2         HG20+Compression
========== ==================

