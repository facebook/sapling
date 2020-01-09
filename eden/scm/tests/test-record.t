#chg-compatible

Set up a repo

  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interactive = true
  > EOF

  $ hg init a
  $ cd a

Record help

  $ hg record -h
  hg record [OPTION]... [FILE]...
  
  interactively select changes to commit
  
      If a list of files is omitted, all changes reported by 'hg status' will be
      candidates for recording.
  
      See 'hg help dates' for a list of formats valid for -d/--date.
  
      If using the text interface (see 'hg help config'), you will be prompted
      for whether to record changes to each modified file, and for files with
      multiple changes, for each change to use. For each query, the following
      responses are possible:
  
        y - record this change
        n - skip this change
        e - edit this change manually
  
        s - skip remaining changes to this file
        f - record remaining changes to this file
  
        d - done, skip remaining changes and files
        a - record all changes to all remaining files
        q - quit, recording no changes
  
        ? - display help
  
      This command is not available when committing a merge.
  
  Options ([+] can be repeated):
  
   -A --addremove           mark new/missing files as added/removed before
                            committing
      --amend               amend the parent of the working directory
   -s --secret              use the secret phase for committing
   -e --edit                invoke editor on commit messages
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
   -d --date DATE           record the specified date as commit date
   -u --user USER           record the specified user as committer
   -w --ignore-all-space    ignore white space when comparing lines
   -b --ignore-space-change ignore changes in the amount of white space
   -B --ignore-blank-lines  ignore changes whose lines are all blank
   -Z --ignore-space-at-eol ignore changes in whitespace at EOL
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)

Select no files

  $ touch empty-rw
  $ hg add empty-rw

  $ hg record empty-rw<<EOF
  > n
  > EOF
  diff --git a/empty-rw b/empty-rw
  new file mode 100644
  examine changes to 'empty-rw'? [Ynesfdaq?] n
  
  no changes to record
  [1]

  $ hg tip -p
  changeset:   -1:000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  

With "copy from"

  $ newrepo
  $ cat > A << EOF
  > 1
  > 2
  > 3
  > EOF
  $ hg commit -m A -A A
  $ hg mv A B
  $ cat > B << EOF
  > 0
  > 1
  > 3
  > 5
  > EOF

  $ hg commit -i -m B << EOS
  > y
  > n
  > y
  > y
  > EOS
  diff --git a/A b/B
  rename from A
  rename to B
  3 hunks, 3 lines changed
  examine changes to 'A' and 'B'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
  +0
   1
  record change 1/3 to 'B'? [Ynesfdaq?] n
  
  @@ -1,3 +2,2 @@
   1
  -2
   3
  record change 2/3 to 'B'? [Ynesfdaq?] y
  
  @@ -3,1 +3,2 @@
   3
  +5
  record change 3/3 to 'B'? [Ynesfdaq?] y
  

BUG: '+0' should not be committed.

  $ hg log -r . -p -T '{desc}\n' --git
  B
  diff --git a/A b/B
  rename from A
  rename to B
  --- a/A
  +++ b/B
  @@ -1,3 +1,4 @@
  +0
   1
  -2
   3
  +5
  
