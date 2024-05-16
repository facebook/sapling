#require no-eden jq

Test that different formats of committer date are supported

  $ configure modern
  $ enable commitextras

  $ mkcommit() {
  >   local name="$1"
  >   shift
  >   echo "$name" > "$name"
  >   sl add "$name"
  >   sl commit -q -m "$name" "$@"
  > }

  $ newrepo

The first commit is in the normal Sapling format and only has a date.
  $ mkcommit normal -d '2020-06-01T01:00:00'

The second commit simulates a Git commit imported into Mononoke, which stores the committer
in a single extra, and the author in the main date field.
  $ mkcommit mononoke-git -d '2020-06-01T02:00:00' --extra 'committer=committer <> 1590978600 0'

The third commit simulates a Git commit imported directly into Sapling, which stores both
committer and author separately.  Normally the date is max(committer, author), but we use
another date to make it clear which is being used in each case.
  $ mkcommit sapling-git -d '2020-06-01T03:00:00' \
  >  --extra 'committer=committer <>' --extra 'committer_date=1590982200 0' \
  >  --extra 'author=author <>' --extra 'author_date=1590981300 0'

  $ sl log -T 'D: {date|isodate}   C: {committerdate|isodate}   A: {authordate|isodate}   {desc}\n'
  D: 2020-06-01 03:00 +0000   C: 2020-06-01 03:30 +0000   A: 2020-06-01 03:15 +0000   sapling-git
  D: 2020-06-01 02:00 +0000   C: 2020-06-01 02:30 +0000   A: 2020-06-01 02:00 +0000   mononoke-git
  D: 2020-06-01 01:00 +0000   C: 2020-06-01 01:00 +0000   A: 2020-06-01 01:00 +0000   normal

  $ sl log
  commit:      ba97f51c2ccc
  user:        test
  date:        Mon Jun 01 03:00:00 2020 +0000
  summary:     sapling-git
  
  commit:      6a3f92ef44af
  user:        test
  date:        Mon Jun 01 02:00:00 2020 +0000
  summary:     mononoke-git
  
  commit:      7eaf4579b3e6
  user:        test
  date:        Mon Jun 01 01:00:00 2020 +0000
  summary:     normal

  $ sl log --config log.use-committer-date=true
  commit:      ba97f51c2ccc
  user:        test
  date:        Mon Jun 01 03:30:00 2020 +0000
  summary:     sapling-git
  
  commit:      6a3f92ef44af
  user:        test
  date:        Mon Jun 01 02:30:00 2020 +0000
  summary:     mononoke-git
  
  commit:      7eaf4579b3e6
  user:        test
  date:        Mon Jun 01 01:00:00 2020 +0000
  summary:     normal

  $ sl log -Tjson | jq -c '.[] |
  >  {
  >    date: (if .date == null then null else (.date[0] | todate) end),
  >    author_date: (if .author_date == null then null else (.author_date[0] | todate) end),
  >    committer_date: (if .committer_date == null then null else (.committer_date[0] | todate) end),
  >    desc
  >  }'
  {"date":"2020-06-01T03:00:00Z","author_date":"2020-06-01T03:15:00Z","committer_date":"2020-06-01T03:30:00Z","desc":"sapling-git"}
  {"date":"2020-06-01T02:00:00Z","author_date":null,"committer_date":"2020-06-01T02:30:00Z","desc":"mononoke-git"}
  {"date":"2020-06-01T01:00:00Z","author_date":null,"committer_date":null,"desc":"normal"}

  $ sl log -Tjson --config log.use-committer-date=true | jq -c '.[] |
  >  {
  >    date: (if .date == null then null else (.date[0] | todate) end),
  >    author_date: (if .author_date == null then null else (.author_date[0] | todate) end),
  >    committer_date: (if .committer_date == null then null else (.committer_date[0] | todate) end),
  >    desc
  >  }'
  {"date":"2020-06-01T03:30:00Z","author_date":"2020-06-01T03:15:00Z","committer_date":"2020-06-01T03:30:00Z","desc":"sapling-git"}
  {"date":"2020-06-01T02:30:00Z","author_date":null,"committer_date":"2020-06-01T02:30:00Z","desc":"mononoke-git"}
  {"date":"2020-06-01T01:00:00Z","author_date":null,"committer_date":null,"desc":"normal"}


