#require test-repo slow

Set vars:

  $ CONTRIBDIR="$TESTDIR/../contrib"

Prepare repo-a:

  $ hg init repo-a
  $ cd repo-a

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

  $ cat > .hg/hgrc << EOF
  > [extensions]
  > perfstatusext=$CONTRIBDIR/perf.py
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
   perfrevlog    (no help text available)
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
  $ filter_perf_output () {
  >     egrep -v 'wall' || true
  > }
  $ hg perfaddremove 2>&1 | filter_perf_output
  $ hg perfancestors 2>&1 | filter_perf_output
  $ hg perfancestorset 2 2>&1 | filter_perf_output
  $ hg perfannotate a 2>&1 | filter_perf_output
  ! result: 3
  $ hg perfbranchmap 2>&1 | filter_perf_output
  ! base
  ! immutable
  ! served
  ! visible
  ! None
  $ hg perfcca 2>&1 | filter_perf_output
  ! result: <mercurial.scmutil.casecollisionauditor object at 0x*> (glob)
  $ hg perfchangeset 2 2>&1 | filter_perf_output
  $ hg perfctxfiles 2 2>&1 | filter_perf_output
  $ hg perfdiffwd 2>&1 | filter_perf_output
  ! diffopts: none
  ! diffopts: -w
  ! diffopts: -b
  ! diffopts: -B
  ! diffopts: -wB
  $ hg perfdirfoldmap 2>&1 | filter_perf_output
  $ hg perfdirs 2>&1 | filter_perf_output
  $ hg perfdirstate 2>&1 | filter_perf_output
  $ hg perfdirstatedirs 2>&1 | filter_perf_output
  $ hg perfdirstatefoldmap 2>&1 | filter_perf_output
  $ hg perfdirstatewrite 2>&1 | filter_perf_output
  $ hg perffncacheencode 2>&1 | filter_perf_output
  $ hg perffncacheload 2>&1 | filter_perf_output
  $ hg perffncachewrite 2>&1 | filter_perf_output
  transaction abort!
  rollback completed
  $ hg perfheads 2>&1 | filter_perf_output
  $ hg perfindex 2>&1 | filter_perf_output
  $ hg perfloadmarkers 2>&1 | filter_perf_output
  $ hg perflog 2>&1 | filter_perf_output
  $ hg perflookup 2 2>&1 | filter_perf_output
  ! result: 20
  $ hg perfmanifest 2 2>&1 | filter_perf_output
  $ hg perfmergecalculate -r 3 2>&1 | filter_perf_output
  $ hg perfmoonwalk 2>&1 | filter_perf_output
  $ hg perfnodelookup 2 2>&1 | filter_perf_output
  $ hg perfpathcopies 1 2 2>&1 | filter_perf_output
  $ hg perfrawfiles 2 2>&1 | filter_perf_output
  $ hg perfrevlog .hg/store/data/a.i 2>&1 | filter_perf_output
  $ hg perfrevrange 2>&1 | filter_perf_output
  $ hg perfrevset 'all()' 2>&1 | filter_perf_output
  $ hg perfstartup 2>&1 | filter_perf_output
  $ hg perfstatus 2>&1 | filter_perf_output
  $ hg perftags 2>&1 | filter_perf_output
  ! result: 1
  $ hg perftemplating 2>&1 | filter_perf_output
  $ hg perfvolatilesets 2>&1 | filter_perf_output
  ! bumped
  ! divergent
  ! extinct
  ! obsolete
  ! suspended
  ! unstable
  ! base
  ! immutable
  ! served
  ! visible
  $ hg perfwalk 2>&1 | filter_perf_output
  ! result: 1

perf parents needs a bigger repo, use the main repo
  $ hg perfparents \
  > --config extensions.perfstatusext=$CONTRIBDIR/perf.py \
  > -R $TESTDIR/.. 2>&1 |grep -v 'obsolete feature' | filter_perf_output

