#chg-compatible

  $ setconfig extensions.treemanifest=!
#require test-repo

Set vars:

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ CONTRIBDIR="$TESTDIR/../contrib"

Prepare repo:

  $ hg init

  $ echo this is file a > a
  $ hg add a
  $ hg commit -m first

  $ echo adding to file a >> a
  $ hg commit -m second

  $ echo adding more to file a >> a
  $ hg commit -m third

  $ hg up -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo merge-this >> a
  $ hg commit -m merge-able

  $ hg up -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

perfstatus

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > perfstatusext=$CONTRIBDIR/perf.py
  > [perf]
  > presleep=0
  > stub=on
  > parentscount=1
  > EOF
  $ hg help perfstatusext
  perfstatusext extension - helper extension to measure performance
  
  Commands:
  
   perfaddremove
                 (no help text available)
   perfancestors
                 (no help text available)
   perfancestorset
                 (no help text available)
   perfannotate  (no help text available)
   perfbdiff     benchmark a bdiff between revisions
   perfbookmarks
                 benchmark parsing bookmarks from disk to memory
   perfbundleread
                 Benchmark reading of bundle files.
   perfcca       (no help text available)
   perfchangegroupchangelog
                 Benchmark producing a changelog group for a changegroup.
   perfchangeset
                 (no help text available)
   perfctxfiles  (no help text available)
   perfdatapack  (no help text available)
   perfdiffwd    Profile diff of working directory changes
   perfdirfoldmap
                 (no help text available)
   perfdirs      (no help text available)
   perfdirstate  (no help text available)
   perfdirstatedirs
                 (no help text available)
   perfdirstatefoldmap
                 (no help text available)
   perfdirstatewrite
                 (no help text available)
   perffncacheencode
                 (no help text available)
   perffncacheload
                 (no help text available)
   perffncachewrite
                 (no help text available)
   perfheads     (no help text available)
   perfindex     (no help text available)
   perfloadmarkers
                 benchmark the time to parse the on-disk markers for a repo
   perflog       (no help text available)
   perflookup    (no help text available)
   perflrucachedict
                 (no help text available)
   perfmanifest  (no help text available)
   perfmergecalculate
                 (no help text available)
   perfmoonwalk  benchmark walking the changelog backwards
   perfnodelookup
                 (no help text available)
   perfparents   (no help text available)
   perfpathcopies
                 (no help text available)
   perfphases    benchmark phasesets computation
   perfrawfiles  (no help text available)
   perfrevlogchunks
                 Benchmark operations on revlog chunks.
   perfrevlogindex
                 Benchmark operations against a revlog index.
   perfrevlogrevision
                 Benchmark obtaining a revlog revision.
   perfrevlogrevisions
                 Benchmark reading a series of revisions from a revlog.
   perfrevrange  (no help text available)
   perfrevset    benchmark the execution time of a revset
   perfstartup   (no help text available)
   perfstatus    (no help text available)
   perftags      (no help text available)
   perftemplating
                 (no help text available)
   perfvolatilesets
                 benchmark the computation of various volatile set
   perfwalk      (no help text available)
   perfwrite     microbenchmark ui.write
  $ hg perfaddremove
  $ hg perfancestors
  $ hg perfancestorset 2
  $ hg perfannotate a
  $ hg perfbdiff -c 1
  $ hg perfbdiff --alldata 1
  $ hg perfbookmarks
  $ hg perfcca
  $ hg perfchangegroupchangelog
  $ hg perfchangeset 2
  $ hg perfctxfiles 2
  $ hg perfdiffwd
  $ hg perfdirfoldmap
  $ hg perfdirs
  $ hg perfdirstate
  $ hg perfdirstatedirs
  $ hg perfdirstatefoldmap
  $ hg perfdirstatewrite
  $ hg perffncacheencode
  $ hg perffncacheload
  $ hg perffncachewrite
  $ hg perfheads
  $ hg perfindex
  $ hg perfloadmarkers
  $ hg perflog
  $ hg perflookup 2
  $ hg perflrucache
  $ hg perfmanifest 2
  $ hg perfmergecalculate -r 3
  $ hg perfmoonwalk
  $ hg perfnodelookup 2
  $ hg perfpathcopies 1 2
  $ hg perfrawfiles 2
  $ hg perfrevlogindex -c
  $ hg perfrevlogrevisions .hg/store/data/a.i
  $ hg perfrevlogrevision -m 0
  $ hg perfrevlogchunks -c
  $ hg perfrevrange
  $ hg perfrevset 'all()'
  $ hg perfstartup
  $ hg perfstatus
  $ hg perftags
  $ hg perftemplating
  $ hg perfvolatilesets
  $ hg perfwalk
  $ hg perfparents
