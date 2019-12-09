#chg-compatible

  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

generaldelta to generaldelta interactions with bundle2 but legacy clients
without changegroup2 support
  $ cat > testcg2.py << EOF
  > from edenscm.mercurial import changegroup, registrar, util
  > import sys
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('testcg2', norepo=True)
  > def testcg2(ui):
  >     if not util.safehasattr(changegroup, 'cg2packer'):
  >         sys.exit(80)
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > testcg2 = $TESTTMP/testcg2.py
  > EOF
  $ hg testcg2 || exit 80

  $ cat > disablecg2.py << EOF
  > from edenscm.mercurial import changegroup, util, error
  > deleted = False
  > def reposetup(ui, repo):
  >     global deleted
  >     if deleted:
  >         return
  >     packermap = changegroup._packermap
  >     # protect against future changes
  >     if len(packermap) != 3:
  >         raise error.Abort('packermap has %d versions, expected 3!' % len(packermap))
  >     for k in ['01', '02', '03']:
  >         if not packermap.get(k):
  >             raise error.Abort("packermap doesn't have key '%s'!" % k)
  > 
  >     del packermap['02']
  >     deleted = True
  > EOF

  $ hginit master
  $ grep generaldelta master/.hg/requires
  generaldelta
  $ cd master
preferuncompressed = False so that we can make both generaldelta and non-generaldelta clones
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > [experimental]
  > bundle2-exp = True
  > [server]
  > preferuncompressed = False
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q --pull --config experimental.bundle2-exp=True
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cd shallow
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > disablecg2 = $TESTTMP/disablecg2.py
  > EOF

  $ cd ../master
  $ echo y > y
  $ hg commit -qAm y

  $ cd ../shallow
  $ hg pull -u
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets d34c38483be9
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ echo a > a
  $ hg commit -qAm a
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
