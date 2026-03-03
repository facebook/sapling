#modern-config-incompatible

#require no-eden

#chg-compatible

==================================
Basic testing for the push command
==================================

Testing of the '--rev' flag
===========================

  $ hg init test-revflag
  $ hg -R test-revflag unbundle "$TESTDIR/bundles/remote.hg"
  adding changesets
  adding manifests
  adding file changes

  $ i=0; for rev in bfaf4b5cbf01 c70afb1ee985 f03ae5a9b979 095cb14b1b4d faa2e4234c7a 21f32785131f 4ce51a113780 93ee6ab32777 916f1afdef90; do
  >    echo
  >    hg init test-revflag-"$i"
  >    hg -R test-revflag push -r "$rev" test-revflag-"$i" --allow-anon
  >    hg -R test-revflag-"$i" verify
  >    i=$((i+1))
  > done
  
  pushing to test-revflag-0
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-3
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-4
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-5
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-6
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-7
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo
  
  pushing to test-revflag-8
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  warning: verify does not actually check anything in this repo

  $ cd test-revflag-8

  $ hg pull ../test-revflag-7
  pulling from ../test-revflag-7

  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cd ..

Test push hook locking
=====================

  $ hg init 1

  $ echo '[ui]' >> 1/.hg/hgrc
  $ echo 'timeout = 10' >> 1/.hg/hgrc

  $ echo foo > 1/foo
  $ hg --cwd 1 ci -A -m foo
  adding foo

  $ hg clone 1 2
  updating to tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone 2 3
  updating to tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat <<EOF > $TESTTMP/debuglocks-pretxn-hook.sh
  > hg debuglocks
  > true
  > EOF
  $ echo '[hooks]' >> 2/.hg/hgrc
  $ echo "pretxnchangegroup.a = sh $TESTTMP/debuglocks-pretxn-hook.sh" >> 2/.hg/hgrc
  $ echo 'changegroup.push = hg push -qf --allow-anon ../1' >> 2/.hg/hgrc

  $ echo bar >> 3/foo
  $ hg --cwd 3 ci -m bar

  $ hg --cwd 3 push --allow-anon ../2
  pushing to ../2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  lock:          user *, process * (*s) (glob)
  wlock:         user *, process * (*s) (glob)
  undolog/lock:  absent
  prefetchlock:  free
  infinitepushbackup.lock: free

  $ hg --cwd 1 debugstrip tip -q
  $ hg --cwd 2 debugstrip tip -q
  $ hg --cwd 3 push --allow-anon ../2 # bundle2+
  pushing to ../2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  lock:          user *, process * (*s) (glob)
  wlock:         user *, process * (*s) (glob)
  undolog/lock:  absent
  prefetchlock:  free
  infinitepushbackup.lock: free

Test bare push with multiple race checking options
--------------------------------------------------

  $ hg init test-bare-push-no-concurrency
  $ hg init test-bare-push-unrelated-concurrency
  $ hg -R test-revflag push -r 'desc(0.0)' test-bare-push-no-concurrency --config server.concurrent-push-mode=strict --allow-anon
  pushing to test-bare-push-no-concurrency
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg -R test-revflag push -r 'desc(0.0)' test-bare-push-unrelated-concurrency --config server.concurrent-push-mode=check-related --allow-anon
  pushing to test-bare-push-unrelated-concurrency
  searching for changes
  adding changesets
  adding manifests
  adding file changes

