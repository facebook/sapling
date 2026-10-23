
#require no-eden


  $ eagerepo
GNU diff is the reference for all of these results.

Prepare tests:

  $ setconfig alias.ndiff='diff --nodates'

  $ sl init repo
  $ cd repo
  $ printf 'hello world\ngoodbye world\n' >foo
  $ sl ci -Amfoo -ufoo
  adding foo


Test added blank lines:

  $ printf '\nhello world\n\ngoodbye world\n\n' >foo

>>> two diffs showing three added lines <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,5 @@
  +
   hello world
  +
   goodbye world
  +
  $ sl ndiff -b
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

  $ sl ndiff -B
  $ sl ndiff -Bb


Test added horizontal space first on a line():

  $ printf '\t hello world\ngoodbye world\n' >foo

>>> four diffs showing added space first on the first line <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ sl ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +	 hello world
   goodbye world

  $ sl ndiff -Bb
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

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +hello world	 
   goodbye world

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +hello world	 
   goodbye world

>>> no diffs <<<

  $ sl ndiff -b
  $ sl ndiff -Bb


Test added horizontal space in the middle of a word:

  $ printf 'hello world\ngood bye world\n' >foo

>>> four diffs showing space inserted into "goodbye" <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ sl ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +good bye world

  $ sl ndiff -Bb
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

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +goodbye		  	world

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
   hello world
  -goodbye world
  +goodbye		  	world

>>> no diffs <<<

  $ sl ndiff -b
  $ sl ndiff -Bb


Test added blank line with horizontal whitespace:

  $ printf 'hello world\n \t\ngoodbye world\n' >foo

>>> three diffs showing added blank line with horizontal space <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

  $ sl ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

>>> no diffs <<<

  $ sl ndiff -Bb


Test added blank line with other whitespace:

  $ printf 'hello  world\n \t\ngoodbye world \n' >foo

>>> three diffs showing added blank line with other space <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello  world
  + 	
  +goodbye world 

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello  world
  + 	
  +goodbye world 

  $ sl ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  + 	
   goodbye world

>>> no diffs <<<

  $ sl ndiff -Bb


Test whitespace changes:

  $ printf 'helloworld\ngoodbye\tworld \n' >foo

>>> four diffs showing changed whitespace <<<

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  -goodbye world
  +helloworld
  +goodbye	world 

  $ sl ndiff -B
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  -goodbye world
  +helloworld
  +goodbye	world 

  $ sl ndiff -b
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +helloworld
   goodbye world

  $ sl ndiff -Bb
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,2 @@
  -hello world
  +helloworld
   goodbye world

>>> no diffs <<<

  $ sl ndiff -w


Test whitespace changes and blank lines:

  $ printf 'helloworld\n\n\n\ngoodbye\tworld \n' >foo

>>> five diffs showing changed whitespace <<<

  $ sl ndiff
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

  $ sl ndiff -B
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

  $ sl ndiff -b
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

  $ sl ndiff -Bb
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

  $ sl ndiff -w
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

  $ sl ndiff -wB


Test \r (carriage return) as used in "DOS" line endings:

  $ printf 'hello world\r\n\r\ngoodbye\rworld\n' >foo

  $ sl ndiff
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello world\r (esc)
  +\r (esc)
  +goodbye\r (no-eol) (esc)
  world

Test \r (carriage return) as used in "DOS" line endings:

  $ printf 'hello world    \r\n\t\ngoodbye world\n' >foo

  $ sl ndiff --ignore-space-at-eol
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  +\t (esc)
   goodbye world

No completely blank lines to ignore:

  $ printf 'hello world\r\n\r\ngoodbye\rworld\n' >foo

  $ sl ndiff --ignore-blank-lines
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  -goodbye world
  +hello world\r (esc)
  +\r (esc)
  +goodbye\r (no-eol) (esc)
  world

Only new line noticed:

  $ sl ndiff --ignore-space-change
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  +\r (esc)
   goodbye world

  $ sl ndiff --ignore-all-space
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
   hello world
  +\r (esc)
   goodbye world

New line not noticed when space change ignored:

  $ sl ndiff --ignore-blank-lines --ignore-all-space

Do not ignore all newlines, only blank lines

  $ printf 'hello \nworld\ngoodbye world\n' > foo
  $ sl ndiff --ignore-blank-lines
  diff -r 540c40a65b78 foo
  --- a/foo
  +++ b/foo
  @@ -1,2 +1,3 @@
  -hello world
  +hello 
  +world
   goodbye world

Test hunk offsets adjustments with --ignore-blank-lines

  $ sl revert -aC
  reverting foo
  $ printf '\nb\nx\nd\n' > a
  $ printf 'b\ny\nd\n' > b
  $ sl add a b
  $ sl ci -m add
  $ sl cat -r . a > b
  $ sl cat -r . b > a
  $ sl diff -B --nodates a > ../diffa
  $ cat ../diffa
  diff -r 0e66aa54f318 a
  --- a/a
  +++ b/a
  @@ -1,4 +1,4 @@
   
   b
  -x
  +y
   d
  $ sl diff -B --nodates b > ../diffb
FIXME: the hunk should start at +2
  $ cat ../diffb
  diff -r 0e66aa54f318 b
  --- a/b
  +++ b/b
  @@ -1,3 +1,3 @@
   b
  -y
  +x
   d
  $ sl revert -aC
  reverting a
  reverting b
  $ sl import --no-commit ../diffa
  applying ../diffa
  $ sl revert -aC
  reverting a
  $ sl import --no-commit ../diffb
  applying ../diffb
  $ sl revert -aC
  reverting b

Ignored blank lines do not shift the new file offsets of later hunks

  $ printf 'hello\nwhitespace\nmy\nold\nfriend\nsix\nseven\neight\nnine\nten\neleven\ntwelve' > c
  $ sl add c
  $ sl ci -m c

Capitalize the first and last lines and add a blank line after five:

  $ printf 'HELLO\nwhitespace\nmy\nold\nfriend\n\nsix\nseven\neight\nnine\nten\neleven\nTWELVE' > c

>>> one diff showing the blank line moving TWELVE to line 13 <<<

  $ sl ndiff c
  diff -r 036d09193e15 c
  --- a/c
  +++ b/c
  @@ -1,12 +1,13 @@
  -hello
  +HELLO
   whitespace
   my
   old
   friend
  +
   six
   seven
   eight
   nine
   ten
   eleven
  -twelve
  \ No newline at end of file
  +TWELVE
  \ No newline at end of file

>>> two diffs dropping the blank line, with TWELVE still at line 13 <<<

FIXME: the second hunk should start at +10 and end with "\ No newline at end of file"
  $ sl ndiff -B c
  diff -r 036d09193e15 c
  --- a/c
  +++ b/c
  @@ -1,4 +1,4 @@
  -hello
  +HELLO
   whitespace
   my
   old
  @@ -9,4 +9,4 @@
   nine
   ten
   eleven
  -twelve
  \ No newline at end of file
  +TWELVE (no-eol)

FIXME: the second hunk should start at +13 and end with "\ No newline at end of file"
  $ sl ndiff -B -U0 c
  diff -r 036d09193e15 c
  --- a/c
  +++ b/c
  @@ -1,1 +1,1 @@
  -hello
  +HELLO
  @@ -12,1 +12,1 @@
  -twelve
  \ No newline at end of file
  +TWELVE (no-eol)

  $ sl revert -C c

Ignored blank lines inside a joined hunk do not shift the new file offsets of
later hunks either

  $ printf 'one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n' > d
  $ sl add d
  $ sl ci -m d

Capitalize one, four and twelve, and add a blank line after two, inside the
hunk joining one and four:

  $ printf 'ONE\ntwo\n\nthree\nFOUR\nfive\nsix\nseven\neight\nnine\nten\neleven\nTWELVE\n' > d

>>> one diff showing the blank line moving TWELVE to line 13 <<<

  $ sl ndiff d
  diff -r e7a593c8fc19 d
  --- a/d
  +++ b/d
  @@ -1,7 +1,8 @@
  -one
  +ONE
   two
  +
   three
  -four
  +FOUR
   five
   six
   seven
  @@ -9,4 +10,4 @@
   nine
   ten
   eleven
  -twelve
  +TWELVE

>>> one diff dropping the blank line, with TWELVE still at line 13 <<<

FIXME: the second hunk should start at +10
  $ sl ndiff -B d
  diff -r e7a593c8fc19 d
  --- a/d
  +++ b/d
  @@ -1,7 +1,7 @@
  -one
  +ONE
   two
   three
  -four
  +FOUR
   five
   six
   seven
  @@ -9,4 +9,4 @@
   nine
   ten
   eleven
  -twelve
  +TWELVE

  $ sl revert -C d

A hunk ending a file without a newline keeps its marker when the hunk holds
ignored blank lines

  $ printf 'one\ntwo\nthree\nfour' > e
  $ sl add e
  $ sl ci -m e

Add a blank line after one and capitalize the last line, still without a newline:

  $ printf 'one\n\ntwo\nthree\nFOUR' > e

>>> one diff showing the blank line and both markers <<<

  $ sl ndiff e
  diff -r e1765c270327 e
  --- a/e
  +++ b/e
  @@ -1,4 +1,5 @@
   one
  +
   two
   three
  -four
  \ No newline at end of file
  +FOUR
  \ No newline at end of file

>>> two diffs dropping the blank line, keeping both markers <<<

FIXME: the hunk should end with "\ No newline at end of file"
  $ sl ndiff -B e
  diff -r e1765c270327 e
  --- a/e
  +++ b/e
  @@ -1,4 +1,4 @@
   one
   two
   three
  -four
  \ No newline at end of file
  +FOUR (no-eol)

FIXME: the hunk should start at +5 and end with "\ No newline at end of file"
  $ sl ndiff -B -U0 e
  diff -r e1765c270327 e
  --- a/e
  +++ b/e
  @@ -4,1 +4,1 @@
  -four
  \ No newline at end of file
  +FOUR (no-eol)
