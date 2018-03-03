Test case that makes use of the weakness of patience diff algorithm

  $ hg init
  >>> open('a', 'w').write('\n'.join(list('a' + 'x' * 10 + 'u' + 'x' * 30 + 'a\n')))
  $ hg commit -m 1 -A a
  >>> open('a', 'w').write('\n'.join(list('b' + 'x' * 30 + 'u' + 'x' * 10 + 'b\n')))
  $ hg diff
  diff -r f0aeecb49805 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,15 +1,4 @@
  -a
  -x
  -x
  -x
  -x
  -x
  -x
  -x
  -x
  -x
  -x
  -u
  +b
   x
   x
   x
  @@ -40,5 +29,16 @@
   x
   x
   x
  -a
  +u
  +x
  +x
  +x
  +x
  +x
  +x
  +x
  +x
  +x
  +x
  +b
   
