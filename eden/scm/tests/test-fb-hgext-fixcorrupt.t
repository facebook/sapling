#chg-compatible

  $ disable treemanifest
  $ SKIPREMOTEFILELOGCHECK=1
  $ export SKIPREMOTEFILELOGCHECK

The fixcorrupt extension fixes pure revlog-based changelog. It is incompatible
with zstore-baked changelog.d:

  $ setconfig format.use-zstore-commit-data=false

  $ cat > noinline.py << EOF
  > from edenscm.mercurial import revlog
  > revlog.REVLOG_DEFAULT_FLAGS = 0
  > revlog.REVLOG_DEFAULT_VERSION = revlog.REVLOG_DEFAULT_FORMAT
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > noinline=$TESTTMP/noinline.py
  > fixcorrupt=
  > EOF

  $ rebuildrepo() {
  >   cd $TESTTMP
  >   [ -d repo ] && rm -rf repo
  >   hg init repo
  >   cd repo
  >   for i in 1 2 3 4 5; do
  >       echo $i > $i
  >       hg commit -m $i -A $i
  >   done
  > }

  $ cat > rewrite.py <<EOF
  > # change (sys.argv[2]) bytes at the end of (sys.argv[1]) file to zeros
  > import sys
  > path = sys.argv[1]
  > n = int(sys.argv[2])
  > with open(path, 'rb+') as f:
  >     f.seek(0, 2)
  >     filesize = f.tell()
  >     n = min(n, filesize)
  >     f.seek(filesize - n)
  >     f.write(b'\0' * n)
  > EOF

  $ corrupt() {
  >   $PYTHON $TESTTMP/rewrite.py .hg/store/"$1" "$2"
  > }

Nothing wrong

  $ rebuildrepo
  $ hg debugfixcorrupt
  changelog looks okay
  manifest looks okay
  nothing to do
  [1]

Changelog corruption

  $ corrupt 00changelog.d 150
  $ hg verify -q 2>&1 | grep error
  15 integrity errors encountered!
  $ hg debugfixcorrupt --no-dryrun
  changelog: corrupted at rev 2 (linkrev=2)
  manifest: marked corrupted at rev 2 (linkrev=2)
  changelog: will lose 3 revisions
  truncating 00changelog.d from 275 to 110 bytes
  truncating 00changelog.i from 320 to 128 bytes
  manifest: will lose 3 revisions
  truncating 00manifest.d from 264 to 99 bytes
  truncating 00manifest.i from 320 to 128 bytes
  fix completed. re-run to check more revisions.
  $ hg verify -q 2>&1 | grep error
  [1]

Manifest corruption

  $ rebuildrepo
  $ corrupt 00manifest.d 150
  $ cp -R .hg/store .hg/store.bak
  $ hg debugfixcorrupt --no-dryrun
  changelog looks okay
  manifest: corrupted at rev 2 (linkrev=2)
  changelog: will lose 3 revisions
  truncating 00changelog.d from 275 to 110 bytes
  truncating 00changelog.i from 320 to 128 bytes
  manifest: will lose 3 revisions
  truncating 00manifest.d from 264 to 99 bytes
  truncating 00manifest.i from 320 to 128 bytes
  fix completed. re-run to check more revisions.
  $ hg verify -q 2>&1 | grep error
  [1]

Verify backups

  $ cat > sha256.py << EOF
  > import hashlib, sys
  > s = hashlib.sha256()
  > s.update(sys.stdin.read())
  > sys.stdout.write('%s' % s.hexdigest()[:8])
  > EOF

  $ ls .hg/truncate-backups | sort
  *-00changelog.d.backup-byte-110-to-275 (glob)
  *-00changelog.i.backup-byte-128-to-320 (glob)
  *-00manifest.d.backup-byte-99-to-264 (glob)
  *-00manifest.i.backup-byte-128-to-320 (glob)

  $ wc -c .hg/store/00changelog* .hg/store/00manifest* | sort
  *99 .hg/store/00manifest.d (glob)
  *110 .hg/store/00changelog.d (glob)
  *128 .hg/store/00changelog.i (glob)
  *128 .hg/store/00manifest.i (glob)
  *465 total (glob)

  $ for i in 00changelog.i 00changelog.d 00manifest.i 00manifest.d; do
  >   printf '%s: before fix: ' $i
  >   cat .hg/store.bak/$i | $PYTHON sha256.py
  >   printf ', restored from backup: '
  >   cat .hg/store/$i .hg/truncate-backups/*-$i.backup* | $PYTHON sha256.py
  >   printf '\n'
  > done
  00changelog.i: before fix: cd8b27c9, restored from backup: cd8b27c9
  00changelog.d: before fix: 873f2076, restored from backup: 873f2076
  00manifest.i: before fix: 1fbd16af, restored from backup: 1fbd16af
  00manifest.d: before fix: 46207222, restored from backup: 46207222

Changelog.i ends with 0s
  $ rebuildrepo
  >>> with open('.hg/store/00changelog.i', 'ab') as f:
  ...     f.write(b'\0' * 128)
  $ hg debugfixcorrupt
  changelog: corrupted at rev 5 (linkrev=0)
  manifest looks okay
  changelog: will lose 2 revisions
  truncating 00changelog.i from 448 to 320 bytes
  re-run with --no-dryrun to fix.

