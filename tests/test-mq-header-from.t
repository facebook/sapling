
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=true" >> $HGRCPATH
  $ catlog() {
  >     cat .hg/patches/$1.patch | sed -e "s/^diff \-r [0-9a-f]* /diff -r ... /" \
  >                                    -e "s/^\(# Parent \).*/\1/"
  >     hg log --template "{rev}: {desc} - {author}\n"
  > }
  $ runtest() {
  >     echo ==== init
  >     hg init a
  >     cd a
  >     hg qinit
  > 
  > 
  >     echo ==== qnew -U
  >     hg qnew -U 1.patch
  >     catlog 1
  > 
  >     echo ==== qref
  >     echo "1" >1
  >     hg add
  >     hg qref
  >     catlog 1
  > 
  >     echo ==== qref -u
  >     hg qref -u mary
  >     catlog 1
  > 
  >     echo ==== qnew
  >     hg qnew 2.patch
  >     echo "2" >2
  >     hg add
  >     hg qref
  >     catlog 2
  > 
  >     echo ==== qref -u
  >     hg qref -u jane
  >     catlog 2
  > 
  > 
  >     echo ==== qnew -U -m
  >     hg qnew -U -m "Three" 3.patch
  >     catlog 3
  > 
  >     echo ==== qref
  >     echo "3" >3
  >     hg add
  >     hg qref
  >     catlog 3
  > 
  >     echo ==== qref -m
  >     hg qref -m "Drei"
  >     catlog 3
  > 
  >     echo ==== qref -u
  >     hg qref -u mary
  >     catlog 3
  > 
  >     echo ==== qref -u -m
  >     hg qref -u maria -m "Three (again)"
  >     catlog 3
  > 
  >     echo ==== qnew -m
  >     hg qnew -m "Four" 4.patch
  >     echo "4" >4of t
  >     hg add
  >     hg qref
  >     catlog 4
  > 
  >     echo ==== qref -u
  >     hg qref -u jane
  >     catlog 4
  > 
  > 
  >     echo ==== qnew with HG header
  >     hg qnew --config 'mq.plain=true' 5.patch
  >     hg qpop
  >     echo "# HG changeset patch" >>.hg/patches/5.patch
  >     echo "# User johndoe" >>.hg/patches/5.patch
  >     hg qpush 2>&1 | grep 'now at'
  >     catlog 5
  > 
  >     echo ==== hg qref
  >     echo "5" >5
  >     hg add
  >     hg qref
  >     catlog 5
  > 
  >     echo ==== hg qref -U
  >     hg qref -U
  >     catlog 5
  > 
  >     echo ==== hg qref -u
  >     hg qref -u johndeere
  >     catlog 5
  > 
  > 
  >     echo ==== qnew with plain header
  >     hg qnew --config 'mq.plain=true' -U 6.patch
  >     hg qpop
  >     hg qpush 2>&1 | grep 'now at'
  >     catlog 6
  > 
  >     echo ==== hg qref
  >     echo "6" >6
  >     hg add
  >     hg qref
  >     catlog 6
  > 
  >     echo ==== hg qref -U
  >     hg qref -U
  >     catlog 6
  > 
  >     echo ==== hg qref -u
  >     hg qref -u johndeere
  >     catlog 6
  > 
  > 
  >     echo ==== "qpop -a / qpush -a"
  >     hg qpop -a
  >     hg qpush -a
  >     hg log --template "{rev}: {desc} - {author}\n"
  > }

======= plain headers

  $ echo "[mq]" >> $HGRCPATH
  $ echo "plain=true" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  ==== init
  ==== qnew -U
  From: test
  
  0: [mq]: 1.patch - test
  ==== qref
  adding 1
  From: test
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - test
  ==== qref -u
  From: mary
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - mary
  ==== qnew
  adding 2
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - test
  0: [mq]: 1.patch - mary
  ==== qref -u
  From: jane
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -U -m
  From: test
  
  Three
  
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref
  adding 3
  From: test
  
  Three
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -m
  From: test
  
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  From: mary
  
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - mary
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u -m
  From: maria
  
  Three (again)
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -m
  adding 4of
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - test
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  From: jane
  
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with HG header
  popping 5.patch
  now at: 4.patch
  now at: 5.patch
  # HG changeset patch
  # User johndoe
  4: imported patch 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 5
  # HG changeset patch
  # User johndoe
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  # HG changeset patch
  # User test
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - test
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  # HG changeset patch
  # User johndeere
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with plain header
  popping 6.patch
  now at: 5.patch
  now at: 6.patch
  From: test
  
  5: imported patch 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 6
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  From: johndeere
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - johndeere
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qpop -a / qpush -a
  popping 6.patch
  popping 5.patch
  popping 4.patch
  popping 3.patch
  popping 2.patch
  popping 1.patch
  patch queue now empty
  applying 1.patch
  applying 2.patch
  applying 3.patch
  applying 4.patch
  applying 5.patch
  applying 6.patch
  now at: 6.patch
  5: imported patch 6.patch - johndeere
  4: imported patch 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: imported patch 2.patch - jane
  0: imported patch 1.patch - mary
  $ rm -r sandbox

======= hg headers

  $ echo "plain=false" >> $HGRCPATH
  $ mkdir sandbox
  $ (cd sandbox ; runtest)
  ==== init
  ==== qnew -U
  # HG changeset patch
  # User test
  # Parent 
  
  0: [mq]: 1.patch - test
  ==== qref
  adding 1
  # HG changeset patch
  # User test
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - test
  ==== qref -u
  # HG changeset patch
  # User mary
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - mary
  ==== qnew
  adding 2
  # HG changeset patch
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - test
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User jane
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -U -m
  # HG changeset patch
  # User test
  # Parent 
  Three
  
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref
  adding 3
  # HG changeset patch
  # User test
  # Parent 
  Three
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -m
  # HG changeset patch
  # User test
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User mary
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - mary
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u -m
  # HG changeset patch
  # User maria
  # Parent 
  Three (again)
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -m
  adding 4of
  # HG changeset patch
  # Parent 
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - test
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User jane
  # Parent 
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with HG header
  popping 5.patch
  now at: 4.patch
  now at: 5.patch
  # HG changeset patch
  # User johndoe
  4: imported patch 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 5
  # HG changeset patch
  # User johndoe
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  # HG changeset patch
  # User test
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - test
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  # HG changeset patch
  # User johndeere
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with plain header
  popping 6.patch
  now at: 5.patch
  now at: 6.patch
  From: test
  
  5: imported patch 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 6
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  From: johndeere
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - johndeere
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qpop -a / qpush -a
  popping 6.patch
  popping 5.patch
  popping 4.patch
  popping 3.patch
  popping 2.patch
  popping 1.patch
  patch queue now empty
  applying 1.patch
  applying 2.patch
  applying 3.patch
  applying 4.patch
  applying 5.patch
  applying 6.patch
  now at: 6.patch
  5: imported patch 6.patch - johndeere
  4: imported patch 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: imported patch 2.patch - jane
  0: imported patch 1.patch - mary
  $ rm -r sandbox
  $ runtest
  ==== init
  ==== qnew -U
  # HG changeset patch
  # User test
  # Parent 
  
  0: [mq]: 1.patch - test
  ==== qref
  adding 1
  # HG changeset patch
  # User test
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - test
  ==== qref -u
  # HG changeset patch
  # User mary
  # Parent 
  
  diff -r ... 1
  --- /dev/null
  +++ b/1
  @@ -0,0 +1,1 @@
  +1
  0: [mq]: 1.patch - mary
  ==== qnew
  adding 2
  # HG changeset patch
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - test
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User jane
  # Parent 
  
  diff -r ... 2
  --- /dev/null
  +++ b/2
  @@ -0,0 +1,1 @@
  +2
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -U -m
  # HG changeset patch
  # User test
  # Parent 
  Three
  
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref
  adding 3
  # HG changeset patch
  # User test
  # Parent 
  Three
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -m
  # HG changeset patch
  # User test
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - test
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User mary
  # Parent 
  Drei
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Drei - mary
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u -m
  # HG changeset patch
  # User maria
  # Parent 
  Three (again)
  
  diff -r ... 3
  --- /dev/null
  +++ b/3
  @@ -0,0 +1,1 @@
  +3
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew -m
  adding 4of
  # HG changeset patch
  # Parent 
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - test
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qref -u
  # HG changeset patch
  # User jane
  # Parent 
  Four
  
  diff -r ... 4of
  --- /dev/null
  +++ b/4of
  @@ -0,0 +1,1 @@
  +4 t
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with HG header
  popping 5.patch
  now at: 4.patch
  now at: 5.patch
  # HG changeset patch
  # User johndoe
  4: imported patch 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 5
  # HG changeset patch
  # User johndoe
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndoe
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  # HG changeset patch
  # User test
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - test
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  # HG changeset patch
  # User johndeere
  # Parent 
  
  diff -r ... 5
  --- /dev/null
  +++ b/5
  @@ -0,0 +1,1 @@
  +5
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qnew with plain header
  popping 6.patch
  now at: 5.patch
  now at: 6.patch
  From: test
  
  5: imported patch 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref
  adding 6
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -U
  From: test
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - test
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== hg qref -u
  From: johndeere
  
  diff -r ... 6
  --- /dev/null
  +++ b/6
  @@ -0,0 +1,1 @@
  +6
  5: [mq]: 6.patch - johndeere
  4: [mq]: 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: [mq]: 2.patch - jane
  0: [mq]: 1.patch - mary
  ==== qpop -a / qpush -a
  popping 6.patch
  popping 5.patch
  popping 4.patch
  popping 3.patch
  popping 2.patch
  popping 1.patch
  patch queue now empty
  applying 1.patch
  applying 2.patch
  applying 3.patch
  applying 4.patch
  applying 5.patch
  applying 6.patch
  now at: 6.patch
  5: imported patch 6.patch - johndeere
  4: imported patch 5.patch - johndeere
  3: Four - jane
  2: Three (again) - maria
  1: imported patch 2.patch - jane
  0: imported patch 1.patch - mary

  $ cd ..
