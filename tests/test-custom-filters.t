  $ hg init

  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > prefixfilter = prefix.py
  > [encode]
  > *.txt = stripprefix: Copyright 2046, The Masters
  > [decode]
  > *.txt = insertprefix: Copyright 2046, The Masters
  > EOF

  $ cat > prefix.py <<EOF
  > from mercurial import error
  > def stripprefix(s, cmd, filename, **kwargs):
  >     header = '%s\n' % cmd
  >     if s[:len(header)] != header:
  >         raise error.Abort('missing header "%s" in %s' % (cmd, filename))
  >     return s[len(header):]
  > def insertprefix(s, cmd):
  >     return '%s\n%s' % (cmd, s)
  > def reposetup(ui, repo):
  >     repo.adddatafilter('stripprefix:', stripprefix)
  >     repo.adddatafilter('insertprefix:', insertprefix)
  > EOF

  $ cat > .hgignore <<EOF
  > .hgignore
  > prefix.py
  > prefix.pyc
  > EOF

  $ cat > stuff.txt <<EOF
  > Copyright 2046, The Masters
  > Some stuff to ponder very carefully.
  > EOF
  $ hg add stuff.txt
  $ hg ci -m stuff

Repository data:

  $ hg cat stuff.txt
  Some stuff to ponder very carefully.

Fresh checkout:

  $ rm stuff.txt
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat stuff.txt
  Copyright 2046, The Masters
  Some stuff to ponder very carefully.
  $ echo "Very very carefully." >> stuff.txt
  $ hg stat
  M stuff.txt

  $ echo "Unauthorized material subject to destruction." > morestuff.txt

Problem encoding:

  $ hg add morestuff.txt
  $ hg ci -m morestuff
  abort: missing header "Copyright 2046, The Masters" in morestuff.txt
  [255]
  $ hg stat
  M stuff.txt
  A morestuff.txt
