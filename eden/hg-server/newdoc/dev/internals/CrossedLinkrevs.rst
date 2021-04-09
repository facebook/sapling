Crossed Linkrevs
================

When working with filelogs (and manifest logs), it is important to be aware that file revisions may not necessarily appear in an order that corresponds to the changelog. In other words, filelogs may contain "crossed linkrevs". Consider:

* filerevs created with the same content and same parents have the same hash

* newly-created identical files are a common instance of such

* pull will fix up linkrevs when doing a pull where the linked rev is not included (ie on a parallel branch)

* the file revisions come across in their original order, so an old file revision can be fixed up to point to a new cset

* the next linkrev will probably "cross" it by pointing to an earlier revision

The filelog graph itself will continue to be topologically sorted (all ancestors before descendants) as will the changelog graph, but the orderings may be different. Thus, you cannot for instance assume that if you find a filelog with linkrev x, you have found all filelogs with linkrev < x.

If you need to iterate across file revisions in a "changelog-compatible" order, it may often be sufficient to do the following:

.. sourcecode:: python

   visit = range(len(fl)).sort(key=fl.linkrev)
   for r in visit:
       ...


