  $ echo "[extensions]" >> $HGRCPATH
  $ echo "churn=" >> $HGRCPATH

create test repository

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda -u user1 -d 6:00
  adding a
  $ echo b >> a
  $ echo b > b
  $ hg ci -m changeba -u user2 -d 9:00 a
  $ hg ci -Am addb -u user2 -d 9:30
  adding b
  $ echo c >> a
  $ echo c >> b
  $ echo c > c
  $ hg ci -m changeca -u user3 -d 12:00 a
  $ hg ci -m changecb -u user3 -d 12:15 b
  $ hg ci -Am addc -u user3 -d 12:30
  adding c
  $ mkdir -p d/e
  $ echo abc > d/e/f1.txt
  $ hg ci -Am "add d/e/f1.txt" -u user1 -d 12:45 d/e/f1.txt
  $ mkdir -p d/g
  $ echo def > d/g/f2.txt
  $ hg ci -Am "add d/g/f2.txt" -u user1 -d 13:00 d/g/f2.txt


churn separate directories

  $ cd d
  $ hg churn e
  user1      1 ***************************************************************

churn all

  $ hg churn
  user1      3 ***************************************************************
  user3      3 ***************************************************************
  user2      2 ******************************************

churn excluding one dir

  $ hg churn -X e
  user3      3 ***************************************************************
  user1      2 ******************************************
  user2      2 ******************************************

churn up to rev 2

  $ hg churn -r :2
  user2      2 ***************************************************************
  user1      1 ********************************
  $ cd ..

churn with aliases

  $ cat > ../aliases <<EOF
  > user1 alias1
  > user3 alias3
  > not-an-alias
  > EOF

churn with .hgchurn

  $ mv ../aliases .hgchurn
  $ hg churn
  skipping malformed alias: not-an-alias
  alias1      3 **************************************************************
  alias3      3 **************************************************************
  user2       2 *****************************************
  $ rm .hgchurn

churn with column specifier

  $ COLUMNS=40 hg churn
  user1      3 ***********************
  user3      3 ***********************
  user2      2 ***************

churn by hour

  $ hg churn -f '%H' -s
  06      1 *****************
  09      2 *********************************
  12      4 ******************************************************************
  13      1 *****************


churn with separated added/removed lines

  $ hg rm d/g/f2.txt
  $ hg ci -Am "removed d/g/f2.txt" -u user1 -d 14:00 d/g/f2.txt
  $ hg churn --diffstat
  user1           +3/-1 +++++++++++++++++++++++++++++++++++++++++--------------
  user3           +3/-0 +++++++++++++++++++++++++++++++++++++++++
  user2           +2/-0 +++++++++++++++++++++++++++

churn --diffstat with color

  $ hg --config extensions.color= churn --config color.mode=ansi \
  >     --diffstat --color=always
  user1           +3/-1 \x1b[0;32m+++++++++++++++++++++++++++++++++++++++++\x1b[0m\x1b[0;31m--------------\x1b[0m (esc)
  user3           +3/-0 \x1b[0;32m+++++++++++++++++++++++++++++++++++++++++\x1b[0m (esc)
  user2           +2/-0 \x1b[0;32m+++++++++++++++++++++++++++\x1b[0m (esc)


changeset number churn

  $ hg churn -c
  user1      4 ***************************************************************
  user3      3 ***********************************************
  user2      2 ********************************

  $ echo 'with space = no-space' >> ../aliases
  $ echo a >> a
  $ hg commit -m a -u 'with space' -d 15:00

churn with space in alias

  $ hg churn --aliases ../aliases -r tip
  no-space      1 ************************************************************

  $ cd ..


Issue833: ZeroDivisionError

  $ hg init issue-833
  $ cd issue-833
  $ touch foo
  $ hg ci -Am foo
  adding foo

this was failing with a ZeroDivisionError

  $ hg churn
  test      0 
  $ cd ..

Ignore trailing or leading spaces in emails

  $ cd repo
  $ touch bar
  $ hg ci -Am'bar' -u 'user4 <user4@x.com>'
  adding bar
  $ touch foo
  $ hg ci -Am'foo' -u 'user4 < user4@x.com >'
  adding foo
  $ hg log -l2 --template '[{author|email}]\n'
  [ user4@x.com ]
  [user4@x.com]
  $ hg churn -c
  user1            4 *********************************************************
  user3            3 *******************************************
  user2            2 *****************************
  user4@x.com      2 *****************************
  with space       1 **************

Test multibyte sequences in names

  $ echo bar >> bar
  $ hg --encoding utf-8 ci -m'changed bar' -u 'El Niño <nino@x.com>'
  $ hg --encoding utf-8 churn -ct '{author|person}'
  user1           4 **********************************************************
  user3           3 ********************************************
  user2           2 *****************************
  user4           2 *****************************
  El Ni\xc3\xb1o         1 *************** (esc)
  with space      1 ***************

Test --template argument, with backwards compatibility

  $ hg churn -t '{author|user}'
  user1      4 ***************************************************************
  user3      3 ***********************************************
  user2      2 ********************************
  nino       1 ****************
  with       1 ****************
             0 
  user4      0 
  $ hg churn -T '{author|user}'
  user1      4 ***************************************************************
  user3      3 ***********************************************
  user2      2 ********************************
  nino       1 ****************
  with       1 ****************
             0 
  user4      0 
  $ hg churn -t 'alltogether'
  alltogether     11 *********************************************************
  $ hg churn -T 'alltogether'
  alltogether     11 *********************************************************

  $ cd ..
