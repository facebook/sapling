  $ hg init

  $ cat > a.c <<'EOF'
  > /*
  >  * This function returns 1.
  >  */
  > int f() {
  >   return 1;
  > }
  > /*
  >  * This function returns 2.
  >  */
  > int g() {
  >   return 2;
  > }
  > /*
  >  * This function returns 3.
  >  */
  > int h() {
  >   return 3;
  > }
  > EOF

  $ cat > b.c <<'EOF'
  > if (x) {
  >    do_something();
  > }
  > 
  > if (y) {
  >    do_something_else();
  > }
  > EOF

  $ cat > c.rb <<'EOF'
  > #!ruby
  > ["foo", "bar", "baz"].map do |i|
  >   i.upcase
  > end
  > EOF

  $ cat > d.py <<'EOF'
  > try:
  >     import foo
  > except ImportError:
  >     pass
  > try:
  >     import bar
  > except ImportError:
  >     pass
  > EOF

The below two files are taken from git: t/t4061-diff-indent.sh

  $ cat > spaces.txt <<'EOF'
  > 1
  > 2
  > a
  > 
  > b
  > 3
  > 4
  > EOF

  $ cat > functions.c <<'EOF'
  > 1
  > 2
  > /* function */
  > foo() {
  >     foo
  > }
  > 
  > 3
  > 4
  > EOF

  $ hg commit -m 1 -A . -q

  $ cat > a.c <<'EOF'
  > /*
  >  * This function returns 1.
  >  */
  > int f() {
  >   return 1;
  > }
  > /*
  >  * This function returns 3.
  >  */
  > int h() {
  >   return 3;
  > }
  > EOF

  $ cat > b.c <<'EOF'
  > if (x) {
  >    do_something();
  > }
  > 
  > if (y) {
  >    do_another_thing();
  > }
  > 
  > if (y) {
  >    do_something_else();
  > }
  > EOF

  $ cat > c.rb <<'EOF'
  > #!ruby
  > ["foo", "bar", "baz"].map do |i|
  >   i
  > end
  > ["foo", "bar", "baz"].map do |i|
  >   i.upcase
  > end
  > EOF

  $ cat > d.py <<'EOF'
  > try:
  >     import foo
  > except ImportError:
  >     pass
  > try:
  >     import baz
  > except ImportError:
  >     pass
  > try:
  >     import bar
  > except ImportError:
  >     pass
  > EOF

  $ cat > spaces.txt <<'EOF'
  > 1
  > 2
  > a
  > 
  > b
  > a
  > 
  > b
  > 3
  > 4
  > EOF

  $ cat > functions.c <<'EOF'
  > 1
  > 2
  > /* function */
  > bar() {
  >     foo
  > }
  > 
  > /* function */
  > foo() {
  >     foo
  > }
  > 
  > 3
  > 4
  > EOF

  $ hg diff --git
  diff --git a/a.c b/a.c
  --- a/a.c
  +++ b/a.c
  @@ -5,12 +5,6 @@
     return 1;
   }
   /*
  - * This function returns 2.
  - */
  -int g() {
  -  return 2;
  -}
  -/*
    * This function returns 3.
    */
   int h() {
  diff --git a/b.c b/b.c
  --- a/b.c
  +++ b/b.c
  @@ -3,5 +3,9 @@
   }
   
   if (y) {
  +   do_another_thing();
  +}
  +
  +if (y) {
      do_something_else();
   }
  diff --git a/c.rb b/c.rb
  --- a/c.rb
  +++ b/c.rb
  @@ -1,4 +1,7 @@
   #!ruby
   ["foo", "bar", "baz"].map do |i|
  +  i
  +end
  +["foo", "bar", "baz"].map do |i|
     i.upcase
   end
  diff --git a/d.py b/d.py
  --- a/d.py
  +++ b/d.py
  @@ -3,6 +3,10 @@
   except ImportError:
       pass
   try:
  +    import baz
  +except ImportError:
  +    pass
  +try:
       import bar
   except ImportError:
       pass
  diff --git a/functions.c b/functions.c
  --- a/functions.c
  +++ b/functions.c
  @@ -1,6 +1,11 @@
   1
   2
   /* function */
  +bar() {
  +    foo
  +}
  +
  +/* function */
   foo() {
       foo
   }
  diff --git a/spaces.txt b/spaces.txt
  --- a/spaces.txt
  +++ b/spaces.txt
  @@ -3,5 +3,8 @@
   a
   
   b
  +a
  +
  +b
   3
   4
