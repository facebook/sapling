GNU diff is the reference for all of these results.

Prepare tests:

  $ echo '[alias]' >> $HGRCPATH
  $ echo 'ndiff = diff --nodates' >> $HGRCPATH

  $ hg init
  $ printf 'hello world\ngoodbye world\n' >foo
  $ hg ci -Amfoo -ufoo
  adding foo


Test added blank lines:

  $ printf '\nhello world\n\ngoodbye world\n\n' >foo

>>> two diffs showing three added lines <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  +
   hello world
  +
   goodbye world
  +
  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  +
   hello world
  +
   goodbye world
  +

>>> no diffs <<<

  $ hg ndiff -B
  $ hg ndiff -Bb


Test added horizontal space first on a line():

  $ printf '\t hello world\ngoodbye world\n' >foo

>>> four diffs showing added space first on the first line <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ hg ndiff -Bb
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world


Test added horizontal space last on a line:

  $ printf 'hello world\t \ngoodbye world\n' >foo

>>> two diffs showing space appended to the first line <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +hello world	 
   goodbye world

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +hello world	 
   goodbye world

>>> no diffs <<<

  $ hg ndiff -b
  $ hg ndiff -Bb


Test added horizontal space in the middle of a word:

  $ printf 'hello world\ngood bye world\n' >foo

>>> four diffs showing space inserted into "goodbye" <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ hg ndiff -Bb
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world


Test increased horizontal whitespace amount:

  $ printf 'hello world\ngoodbye\t\t  \tworld\n' >foo

>>> two diffs showing changed whitespace amount in the last line <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +goodbye		  	world

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +goodbye		  	world

>>> no diffs <<<

  $ hg ndiff -b
  $ hg ndiff -Bb


Test added blank line with horizontal whitespace:

  $ printf 'hello world\n \t\ngoodbye world\n' >foo

>>> three diffs showing added blank line with horizontal space <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

>>> no diffs <<<

  $ hg ndiff -Bb


Test added blank line with other whitespace:

  $ printf 'hello  world\n \t\ngoodbye world \n' >foo

>>> three diffs showing added blank line with other space <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello  world
  + 	
  +goodbye world 

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello  world
  + 	
  +goodbye world 

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

>>> no diffs <<<

  $ hg ndiff -Bb


Test whitespace changes:

  $ printf 'helloworld\ngoodbye\tworld \n' >foo

>>> four diffs showing changed whitespace <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  -goodbye world
  +helloworld
  +goodbye	world 

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  -goodbye world
  +helloworld
  +goodbye	world 

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +helloworld
   goodbye world

  $ hg ndiff -Bb
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +helloworld
   goodbye world

>>> no diffs <<<

  $ hg ndiff -w


Test whitespace changes and blank lines:

  $ printf 'helloworld\n\n\n\ngoodbye\tworld \n' >foo

>>> five diffs showing changed whitespace <<<

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  -hello world
  -goodbye world
  +helloworld
  +
  +
  +
  +goodbye	world 

  $ hg ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  -hello world
  -goodbye world
  +helloworld
  +
  +
  +
  +goodbye	world 

  $ hg ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  -hello world
  +helloworld
  +
  +
  +
   goodbye world

  $ hg ndiff -Bb
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  -hello world
  +helloworld
  +
  +
  +
   goodbye world

  $ hg ndiff -w
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
   hello world
  +
  +
  +
   goodbye world

>>> no diffs <<<

  $ hg ndiff -wB


Test \r (carriage return) as used in "DOS" line endings:

  $ printf 'hello world\r\n\r\ngoodbye\rworld\n' >foo

  $ hg ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello world\r (esc)
  +\r (esc)
  +goodbye\rworld (esc)

No completely blank lines to ignore:

  $ hg ndiff --ignore-blank-lines
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello world\r (esc)
  +\r (esc)
  +goodbye\rworld (esc)

Only new line noticed:

  $ hg ndiff --ignore-space-change
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  +\r (esc)
   goodbye world

  $ hg ndiff --ignore-all-space
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  +\r (esc)
   goodbye world

New line not noticed when space change ignored:

  $ hg ndiff --ignore-blank-lines --ignore-all-space
