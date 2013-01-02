Test alignment of multibyte characters

  $ HGENCODING=utf-8
  $ export HGENCODING
  $ hg init t
  $ cd t
  $ python << EOF
  > # (byte, width) = (6, 4)
  > s = "\xe7\x9f\xad\xe5\x90\x8d"
  > # (byte, width) = (7, 7): odd width is good for alignment test
  > m = "MIDDLE_"
  > # (byte, width) = (18, 12)
  > l = "\xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d"
  > f = file('s', 'w'); f.write(s); f.close()
  > f = file('m', 'w'); f.write(m); f.close()
  > f = file('l', 'w'); f.write(l); f.close()
  > # instant extension to show list of options
  > f = file('showoptlist.py', 'w'); f.write("""# encoding: utf-8
  > def showoptlist(ui, repo, *pats, **opts):
  >     '''dummy command to show option descriptions'''
  >     return 0
  > cmdtable = {
  >     'showoptlist':
  >         (showoptlist,
  >          [('s', 'opt1', '', 'short width'  + ' %(s)s' * 8, '%(s)s'),
  >           ('m', 'opt2', '', 'middle width' + ' %(m)s' * 8, '%(m)s'),
  >           ('l', 'opt3', '', 'long width'   + ' %(l)s' * 8, '%(l)s')
  >          ],
  >          ""
  >         )
  > }
  > """ % globals())
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
  
  options:
  
   -s --opt1 \xe7\x9f\xad\xe5\x90\x8d         short width \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d \xe7\x9f\xad\xe5\x90\x8d (esc)
   -m --opt2 MIDDLE_      middle width MIDDLE_ MIDDLE_ MIDDLE_ MIDDLE_ MIDDLE_
                          MIDDLE_ MIDDLE_ MIDDLE_
   -l --opt3 \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d long width \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
                          \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
                          \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
  
  use "hg -v help showoptlist" to show the global options


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

add branches/tags

  $ hg branch $S
  marked working directory as branch \xe7\x9f\xad\xe5\x90\x8d (esc)
  (branches are permanent and global, did you want a bookmark?)
  $ hg tag $S
  $ hg branch $M
  marked working directory as branch MIDDLE_
  (branches are permanent and global, did you want a bookmark?)
  $ hg tag $M
  $ hg branch $L
  marked working directory as branch \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d (esc)
  (branches are permanent and global, did you want a bookmark?)
  $ hg tag $L

check alignment of branches

  $ hg tags
  tip                                5:d745ff46155b
  \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d                       4:9259be597f19 (esc)
  MIDDLE_                            3:b06c5b6def9e
  \xe7\x9f\xad\xe5\x90\x8d                               2:64a70663cee8 (esc)

check alignment of tags

  $ hg tags
  tip                                5:d745ff46155b
  \xe9\x95\xb7\xe3\x81\x84\xe9\x95\xb7\xe3\x81\x84\xe5\x90\x8d\xe5\x89\x8d                       4:9259be597f19 (esc)
  MIDDLE_                            3:b06c5b6def9e
  \xe7\x9f\xad\xe5\x90\x8d                               2:64a70663cee8 (esc)

  $ cd ..
