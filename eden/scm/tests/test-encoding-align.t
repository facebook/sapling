#chg-compatible
#debugruntest-compatible

Test alignment of multibyte characters

  $ HGENCODING=utf-8
  $ export HGENCODING
  $ hg init t
  $ cd t
  $ $PYTHON << EOF
  > # (byte, width) = (6, 4)
  > s = b"\xe7\x9f\xad\xe5\x90\x8d"
  > # (byte, width) = (7, 7): odd width is good for alignment test
  > m = b"MIDDLE_"
  > # (byte, width) = (18, 12)
  > l = b"\xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d"
  > f = open('s', 'wb'); _ = f.write(s); f.close()
  > f = open('m', 'wb'); _ = f.write(m); f.close()
  > f = open('l', 'wb'); _ = f.write(l); f.close()
  > # instant extension to show list of options
  > f = open('showoptlist.py', 'wb'); _ = f.write(b"""# encoding: utf-8
  > from edenscm import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command('showoptlist',
  >     [('s', 'opt1', '', 'short width'  + ' %(s)s' * 8, '%(s)s'),
  >     ('m', 'opt2', '', 'middle width' + ' %(m)s' * 8, '%(m)s'),
  >     ('l', 'opt3', '', 'long width'   + ' %(l)s' * 8, '%(l)s')],
  >     '')
  > def showoptlist(ui, repo, *pats, **opts):
  >     '''dummy command to show option descriptions'''
  >     return 0
  > """ % {b's': s, b'm': m, b'l': l})
  > f.close()
  > EOF
  $ S=`cat s`
  $ M=`cat m`
  $ L=`cat l`

alignment of option descriptions in help

  $ cat <<EOF > .hg/hgrc
  > [extensions]
  > ja_ext = `pwd`/showoptlist.py
  > EOF

check alignment of option descriptions in help

  $ hg help showoptlist
  hg showoptlist
  
  dummy command to show option descriptions
  
  Options:
  
   -s --opt1 \xe7\x9f\xad\xe5\x90\x8d         short width \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d (esc)
   -m --opt2 MIDDLE_      middle width MIDDLE_ MIDDLE_ MIDDLE_ MIDDLE_ MIDDLE_
                          MIDDLE_ MIDDLE_ MIDDLE_
   -l --opt3 \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d long width \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
                          \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
                          \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
  
  (some details hidden, use --verbose to show complete help)


  $ rm -f s; touch s
  $ rm -f m; touch m
  $ rm -f l; touch l

add files

  $ cp s $S
  $ hg add $S
  $ cp m $M
  $ hg add $M
  $ cp l $L
  $ hg add $L

commit(1)

  $ echo 'first line(1)' >> s; cp s $S
  $ echo 'first line(2)' >> m; cp m $M
  $ echo 'first line(3)' >> l; cp l $L
  $ hg commit -m 'first commit' -u $S

commit(2)

  $ echo 'second line(1)' >> s; cp s $S
  $ echo 'second line(2)' >> m; cp m $M
  $ echo 'second line(3)' >> l; cp l $L
  $ hg commit -m 'second commit' -u $M

commit(3)

  $ echo 'third line(1)' >> s; cp s $S
  $ echo 'third line(2)' >> m; cp m $M
  $ echo 'third line(3)' >> l; cp l $L
  $ hg commit -m 'third commit' -u $L

check alignment of user names in annotate

  $ hg annotate -u $M
          \xe7\x9f\xad\xe5\x90\x8d: first line(2) (esc)
       MIDDLE_: second line(2)
  \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d: third line(2) (esc)

check alignment of filenames in diffstat

  $ hg diff -c tip --stat
   MIDDLE_      |  1 +
   \xe7\x9f\xad\xe5\x90\x8d         |  1 + (esc)
   \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d |  1 + (esc)
   3 files changed, 3 insertions(+), 0 deletions(-)

add bookmarks

  $ hg book -f $S
  $ hg book -f $M
  $ hg book -f $L

check alignment of bookmarks

  $ hg book
     MIDDLE_                   64a70663cee8
     \xe7\x9f\xad\xe5\x90\x8d                      64a70663cee8 (esc)
   * \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d              64a70663cee8 (esc)
