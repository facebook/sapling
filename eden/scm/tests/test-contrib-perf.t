#chg-compatible

  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
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

  $ hg up -r 'desc(first)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo merge-this >> a
  $ hg commit -m merge-able

  $ hg up -r 'desc(third)'
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
  $ hg perfaddremove
  $ hg perfancestors
  $ hg perfancestorset 'desc(third)'
  $ hg perfannotate a
  $ hg perfbookmarks
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
  $ hg perflog
  $ hg perflookup 2
  $ hg perflrucache
  $ hg perfmanifest 'desc(third)'
  $ hg perfmergecalculate -r 8401ab48b23f93e6592bba6c753148a61f35d5c9
  $ hg perfpathcopies 'desc(second)' 'desc(third)'
  $ hg perfrawfiles 2
  $ hg perfrevrange
  $ hg perfrevset 'all()'
  $ hg perfstartup
  $ hg perfstatus
  $ hg perftemplating
  $ hg perfwalk
  $ hg perfparents
