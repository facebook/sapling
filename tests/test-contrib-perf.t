#require test-repo

Set vars:

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
  created new head

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
  
  list of commands:
  
   perfaddremove
                 (no help text available)
   perfancestors
                 (no help text available)
   perfancestorset
                 (no help text available)
   perfannotate  (no help text available)
   perfbranchmap
                 benchmark the update of a branchmap
   perfcca       (no help text available)
   perfchangeset
                 (no help text available)
   perfctxfiles  (no help text available)
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
   perfrawfiles  (no help text available)
   perfrevlog    Benchmark reading a series of revisions from a revlog.
   perfrevlogrevision
                 Benchmark obtaining a revlog revision.
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
  
  (use "hg help -v perfstatusext" to show built-in aliases and global options)
  $ hg perfaddremove
  $ hg perfancestors
  $ hg perfancestorset 2
  $ hg perfannotate a
  $ hg perfbranchmap
  $ hg perfcca
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
  $ hg perfrevlog .hg/store/data/a.i
  $ hg perfrevlogrevision -m 0
  $ hg perfrevrange
  $ hg perfrevset 'all()'
  $ hg perfstartup
  $ hg perfstatus
  $ hg perftags
  $ hg perftemplating
  $ hg perfvolatilesets
  $ hg perfwalk
  $ hg perfparents

