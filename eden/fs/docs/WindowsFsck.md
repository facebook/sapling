# Windows FSCK

This was written in first person. I here refers to kmancini.

When Eden starts up on all platforms, we have an optional FSCK
(**F**ile **S**ystem **C**hec**K**) that makes sure Eden’s internal on-disk
state is not corrupted (think incorrect `hg status`) and fix it where possible.

On macOS and Linux this check is optional. We can skip it if Eden shutdown
cleanly. However, it is **not optional on Windows** for two reasons. Reason
#1: the notifications about files changing we get from the operating system on
Windows are asynchronous. This means Eden might miss a file change before
stopping. Reason #2: on Windows files can be modified while Eden isn’t running.

Because FSCK isn’t optional on Windows, it causes pain more often for users.
Additionally, Windows FSCK has to perform special checks to handle edits to
files while eden wasn’t running.

FSCK has a history of being really slow and incorrect, and in the summer of
2022, Durham consulted on the Eden team to rewrite FSCK.

The main goal was to handle asynchronous writes to Eden on disk storage.
Notifications from the operating system about files changing were always
asynchronous on Windows. So FSCK was already prepared to handle Eden completely
missing modifications to files. But last year we rolled out the BufferedOverlay
([post](https://fb.workplace.com/groups/edenfs/permalink/2004171086419780/)).
This makes Eden faster in the hot path by buffering writes to disk in-memory
but can mean that Eden can exit with partially persisted internal state. FSCK
needed to be updated to handle this extra case of asynchrony.

Because FSCK was already slow, careful work was done to limit the number of
filesystem operations per file to 1. FSCK had grown bloat over the years as we
patched it to handle various corruption cases, and all the interaction with the
filesystem made it really slow.

However, even with this revamp FSCK did not always leave Eden in sync with the
filesystem, and it did not address FSCK being generally slow.

## How was it broken

Back in 2022 Xavier noticed that files named to a different casing (ex
edenserver.cpp → EdenServer.cpp) would incorrectly appear in `hg status` after
an Eden restart.

This was fixed last fall by ignoring case in FSCK like we do everywhere else on
Windows, but unearthed multiple other bugs:

* **“renamed-ness” of files was ignored by FSCK.** This means that if FSCK missed
a rename it might bring back the old location of the file, the file might have
the wrong contents, or `hg status` would report incorrect information for these
files.
* Removed files could be brought back by FSCK, and `hg status` could report
incorrect information about them.

#### Here’s the nity-grity of how FSCK was incorrect for renames:

| File renamed while eden is …| Old parent is …|File is ...|Same named file in source in scm?|Same named file in destination in scm ?|Old parent is empty |Tombstone placed?|Source inode state after fsck|Source FS state after fsck|Destination inode state after fsck|Destination FS state after fsck|In sync|No re-appearing files|Matches sparse profile behavior|
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
|running|placeholder / full|placeholder|y|y|y/n|y|No inode|No file on disk|inode with destination scm contents|file with source scm contents|❌ (1)|✔️|❌ (2)|
|running|placeholder / full|placeholder|n|y|y/n|y|No inode|No file on disk|inode with destination scm contents|error reading file|❌ (1)|✔️|❌ (2)|
|running|placeholder / full|placeholder|y|n|y/n|y|No inode|No file on disk|inode with source scm contents|inode with source scm contents|✔️|✔️|❌ (2)|
|running|placeholder / full|placeholder|n|n|y/n|y|No inode|No file on disk|error when accessing inode|error when reading file|✔️|✔️|❌ (2)|
|running|placeholder|hydrated placeholder / full|y/n|y/n|y/n|y|No inode|No file on disk|inode with moved contents|file with moved contents|✔️|✔️|✔️|
|running|full|full|y|y/n|y|n|No inode|No file on disk|inode with moved contents|file with moved contents|✔️|✔️|✔️|
|running|full|full|y|y/n|n|n|Inode with scm hash|No file on disk|inode with moved contents|inode with moved contents|❌ (4)|❌ (4)|✔️|
|running|full|full|n|y/n|y/n|n|No inode|No file on disk|inode with moved contents|inode with moved contents|✔️|✔️|✔️|
|stopped|placeholder|full|y|y/n|y/n|n|inode with scm hash|file on disk with scm hash|inode with moved contents|inode with moved contents|✔️|❌(4)|✔️||
stopped|placeholder|full|n|y/n|y/n|n|no inodes|no file on disk|inode with moved contents|inode with moved contents|✔️|✔️|✔️|
|stopped|placeholder / full|placeholder|y/n|y/n|y/n|y/n|inode with source scm hash|file on disk with scm content|inode with destination scm contents|file with source scm contents|❌ (1)|❌ (3)|✔️|
|stopped|placeholder / full|placeholder|y|n|y/n|n|inode with source scm hash|file on disk with scm content|inode with source scm contents|file with source scm contents|✔️|❌ (3)|❌ (2)|
|stopped|placeholder / full|placeholder|n|n|y/n|n|no inode|no file on disk|inode with source scm contents | inode with source scm contents|✔️|✔️|❌(2)|
|stopped|placeholder / full|placeholder|n|y|y/n|n|No inode|No file on disk|inode with destination scm contents|file with source scm contents|❌(1)|✔️|✔️|
|stopped|full|full|y|y/n|y/n|n|Inode with scm hash|No file on disk|inode with moved contents|inode with moved contents|❌(4)|❌(4)|✔️|
|stopped|full|full|n|y/n|y/n|n|No inode|No file on disk|inodes with moved contents|inode with moved contents|✔️|✔️|✔️|

* Checks are good behavior; Xs are bad behavior.
* The most incorrect Eden behavior corresponds to the X in the first check/x
column. Out of sync means `hg status` will be wrong and `hg checkout` is likely
to fail in weird ways.
* The other Xs indicates Eden does something unexpected. Unexpected includes
bringing back removed files or contents being different than they would be on
unix platforms or sparse profiles.

As you can see there are more rows with Xs than without. Generally, the no-X
rows are a bit more common case, but I am sure there are plenty of users who
hit these Xs regularly. Though there are lots of incorrect rows, the issues can
be categorized into 4 root cause bugs. The cause of each X is labeled with the
identified issues below.

Before I get into it though. We need to get on the same page about some terms.

* “full”: This is a ProjectedFS term. For files it means a file is locally
modified or locally created. For directories it only includes locally created
directories.
* “hydrated placeholder”: This is a ProjectedFS term. For files this means the
file has been read, and its contents are present on disk (in your repo
directory). Directories are never hydrated.
* “placeholder”: This is a ProjectedFS term. For files this is a file that has
never been written or read. For directories, this is all directories that were
not locally created (with like mkdir or something).
* “materialized”: This is an Eden term. On windows it generally means disk (in
your repo directory) is the source of truth for this file/directory. Reads for
these files are completed by reading the file off disk.
* “WCP”: This is a mercurial term. Short for “working copy parent”. This is the
last commit that you checked out.


1. **FSCK is unaware that renamed files are special snowflakes in Eden’s model.**

Renamed files are the only files that are placeholders, but their path does not
map to a source control object in the WCP. FSCK does not properly handle this
“exception to the rule case”.

FSCK splits files into two categories full and not-full. FSCK makes sure full
files are materialized and not-full files are represented by the correct source
control object inside Eden. FSCK assumes that not-full files must directly map
to a source control object. This is generally true: when the file content is not
present on disk, ProjectedFS is going to ask Eden for the file, and Eden will
read the path out of a source control object for the WCP. However, if
ProjectedFS were to ask us to read a renamed file, it would ask us with the
original path of the file. So, we would return the source control object at the
original path. In essence, FSCK needs to know to check for the source control
object of a renamed file at the original path instead of the current one.
Though there is a bit of a simpler solution. I’ll go through the solution later.

2. **Moved files are incorrectly handled generally by Windows Eden.**

Like I mentioned above Eden always reads file content out of the WCP. That
means that for renamed files we read their content from the original path in
the WCP. This is a problem if you checked out a new commit since you renamed
the file. We will read the contents of the file at the new checked out commit
instead of the one that was checked out when the rename happened. Worse(?) if
the file was removed in the new commit, you checkout, you will get an internal
error when reading the file!!

This is easier to understand with an example. This little repro will do it:

```
rm fbcode/eden/fs/fuzz/facebook/TARGETS
hg addremove
hg commit -m "remove a file"
hg prev
mv fbcode/eden/fs/fuzz/facebook/TARGETS tmp
hg next
cat tmp #Get-Content: An internal error occurred. : 'C:\open\tfbsource\tmp'
# Bad Eden!!
```

3. **Deleted files are not handled correctly when the parent is a placeholder.**

Rename removes a file from the source location and adds it to the destination.
Removing the file from the source location is subject to the same bugs we have
with removed files.

This issue is a little easier to talk about when there are fewer moving parts
so I’ll describe it below.

**4. Deleted files are not handled correctly when the parent is a full file.**
same as 3.


#### Here’s the nity-grity of how FSCK was incorrect for removals:

|File removed while eden is …|Parent is …|Same named file in scm?|Parent is empty (in both inode and on disk)|Tombstone placed?|Indode state after fsck|Fs state after fsck|In sync|No re-appearing files|
|---|---|---|---|---|---|---|---|---|
|running|placeholder|y/n|y/n|y|No inode|No file on disk|✔️|✔️|
|running|full|y|y|n|No inode|No file on disk|✔️|✔️|
|running|full|y|n|n|Inode with scm hash|No file on disk|❌ (4)|❌ (4)|
|running|full|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|placeholder|y|y/n|n|inode with scm hash|file on disk with scm content|✔️| ❌ (3)|
|stopped|placeholder|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|y|y/n|n|Inode with scm hash|No file on disk|❌ (4)|❌ (4)|
|stopped|full|n|y/n|n|No inode|No file on disk|✔️|✔️|

- the 3rd and 7th rows are more problematic that the 5th. Since these cases are
corruption rather than unexpected behavior.

3. **Deleted files are not handled correctly when the parent is a placeholder.**

First, we have to start with how directories work in ProjectedFS. When you read
a placeholder directory ProjectedFS always makes a “readdir” request to Eden.
ProjectedFS takes the returned “readdir” result from Eden adds locally created
filesand subtracts a thing called tombstones. Tombstones are simply special
files that act as markers that a file was removed.

This behavior is supposed to make it possible for Eden to add and remove files
from directories. But is has a few unfortunate consequences including this bug.

Normally, when a file is removed from a Placeholder, ProjectedFS will place a
tombstone there. However, if a file is removed while Eden isn’t running, no
placeholder is put in its place. Why ... that part I cannot reverse engineer.
Seems like a bug that has *** reasons *** behind it. Perhaps we should ask
Microsoft if they could do better. But anyways this causes a bit of an issue.

After Eden restarts files removed from placeholder directories will
automagically be brought back by ProjectedFS because ProjectedFS doesn’t know
to subtract the file from the directory listing returned from Eden.

To compensate for ProjectedFS’s behavior, Eden matches projectedFS’s behavior
and brings the file back. This is ok-ish. But to be honest we are not bringing
back the file to the state it was in before it was removed. So, to me this is
just a ghost file coming back from the dead. Bad Eden!

**4. Deleted files are not handled correctly when the parent is a full file.**
Based on ProjectedFS forcing our hand a bit on removed files in placeholder
directories, we decided that if a file is missing from disk (without a
tombstone), Eden should bring it back (I’m making a simplification).

However, ProjectedFS doesn’t have the same behavior for full directories as
placeholder directories. ProjectedFS will not revive such removed files on
disk. Eden brings back removed files internally, but they will remain missing
from disk. Very bad Eden!!

## Solutions

### Problem 1

To recap this problem is that FSCK thinks it should be correcting renamed
files to make them match the source control object their path maps too. But
really it should be making them match the source control object at their
original path.

However, I mentioned there is an even simpler solution. It's this: Instead
of even matching the file to a source control object, its sufficient to just
make the inode materialized in Eden. When materialized inodes are read
internally Eden reads it from disk (in your repo directory). Or when you (or
more likely Buck) ask for the sha1 of a file, Eden reads the file from disk
and hashes the contents (with some caching layers in between). This reading
from disk thing is a little spooky. But a lot of Eden is tangled up in this
reading from disk situation, so to avoid the web of issues from growing, we
are gonna accept that reading files from disk is the reality of Eden on Windows.

The reading from disk thing does give us an advantage here. All we have to do
is mark renamed files materialized, and then we know that when Eden goes to
read the file, it will read from disk, and ProjectedFS will ask Eden for the
file at the original path and Eden will look it up the old path in the WCP.
This materialize renamed files is what Eden already does when ProjectedFS tells
us about a rename, so we just need to make FSCK do it too.

So, in summary a solution here is to just make sure all renamed files are
correctly marked materialized in Eden’s internal state.

There are other solutions, but they involve overhauling Eden’s general
treatment of renamed files which as I will get into in the Problem 2 section is
messy to say the least. So we decided to go with this solution.

To make this solution work Eden needs to detect a renamed file in FSCK. Lucky
for us ProjectedFS sets a certain bit in the reparse point representing the
file when its renamed (it also puts the original path in there too, but the
bit is easier to use). This certain bit is not documented, but it is reliable,
and we have a pretty comprehensive suite of tests now that assert our
assumptions about the bit.

Unluckily though, reading a reparse buffer is pretty slow, and it makes our
strict rule of 1 filesystem operation per file two operations per file. And
this has consequences. FSCK gets 2x slower. FSCK was already in the
multi-minutes for users. So 2x here really hurts.

But it so happens this was the kick in the pants we needed to do something
about how slow FSCK was. The way FSCK roughly works is crawl all the tracked
files on disk and in Eden’s representation of them. Then fix each file as
needed. Single threaded.

 Xavier added multiple threads to FSCK, and bam reasonable start up times
 (the rollout is in progress and final numbers will grace your workplace
 feed soon :) )

Now FSCK is typically in the tens of seconds range, and that 2x doesn’t hurt
so bad. We are currently rolling out detect a renames and mark the inode as
materialized solution.


### Problem 2

To recap, this problem is that Eden always serves ProjectedFS read requests
directly from the WCP and that interacts bad with renamed files. To match
sparse profiles behavior, and Eden behavior on macOS and Linux, Eden really
should be reading the contents from the source control tree for the commit
that was checked out when the file was renamed. Or something that matches that
behavior.

Alright so what are the options.

1.  **Do away with this whole read from source control objects and use our
inodes.** This unfortunately doesn’t work so well. This is how Eden use to
work, and there were three+ problems. issue a: ProjectedFS is going to ask
Eden to read the original path of renamed files, and this won’t exist in the
inodes. Eden would need to keep some mapping of renames ... see potential
solution #2 for why an exploration of why this is bad. issue b: This causes
ProjectedFS to over zealously create tombstones and cause lots of weird
behavior. issue c: Edens inode state has had a lot of reliability issues on
windows, so reading from source control is more reliable. See the diff
changing diff D32022639 and [thread](http://xavier%20deguillard%20https//github.com/microsoft/ProjFS-Managed-API/issues/68)
with Microsoft for more details. Overall, we would be adding more problems
than solving to go back to inodes.

2. **Track renames in Eden and special case reading renamed files.** This
“tracking” would be internal Eden state that can fall out of sync with reality
(i.e. ProjectedFS). And the root root (this duplication is not a typo) cause of
all these problems I am writing about in this post is really that we duplicate
state in Eden that gets our of sync with the source of truth. So, adding more
duplicated state to fix our issue of duplicated state (in my opinion) is a bad
idea.

3. _Make all renamed files full on disk._ Now this sounds bad, but hear me out,
it might not be so bad. So first I have to explain that we already materialize
all renamed files. For correctness reasons, Eden has to mark any renamed file
materialized in Eden. Generally, Eden’s “materialized” matches ProjectedFS’s
“full”.  So we already do the Eden equivalent of “Make all renamed files full
on disk.” Renamed files is a case where materialized and full do not match up.
Lining these two up more is not such a bad idea. Additionally, we really are
only talking about files here, not directories. Placeholder directories cannot
be renamed in ProjectedFS. ProjectedFS just strait up refuses to let you rename
placeholder directories - you have to manually copy and delete (honestly thank
goodness, because we can barely handle file renames 😅) . Since we are only
talking about files here, we are not talking about crawling anything here,
it’s just marking single files as full. However, the biggest problem here is
that this will be inherintly racy. “Marking files full” means issue a write to
the file on disk. There will be some period when a file is renamed, but not
full. And theoretically the bug would still exist in that period. Plus, there
could be problems with writing the wrong thing to disk. Overall, I think this
is ok, but not an ideal solution.

4. _Store the source control object in the reparse point._ We can add custom
data to ProjectedFs’s on disk representation for placeholders. We can use that
custom data to include the source control object hash and use that hash to
directly look up the right source control object for a file when asked to read
it. This would be complicated. The reparse buffer storage thing seems sketchy
— I think we have had reliability issues with it in the past. And it could have
performance implications broadly for Eden. However, this seems like the least
bad solution I can come up with.


Generally, I think we should go with 3 or 4 here. 3 will be simpler, but 4 is
more solid. Mark has mentioned he is interested, but he is looking at other
things first. At some point one of the Eden folks will take a look. But right
now we are all dealing with more egregious bugs like problem 4 :). And with
that, onto problem 3 and 4 ...


### Problem 3 & 4

To recap the problems here are that we try to resurrect deleted files to
match ProjectedFS behavior for placeholder directories. Here are the options:

1.  **We could keep trying to match what ProjectedFS does for placeholders.**
Essentially, resurrecting files both on disk and in Eden’s state when they are missing. This would be this kind of behavior:

|File removed while eden is …|Parent is …|Same named file in scm?|Parent is empty (in both inode and on disk)|Tombstone placed?|Inode state after fsck|Fs state after fsck|In sync|No re-appearing files|
|---|---|---|---|---|---|---|---|---|
|running|placeholder|y/n|y/n|y|No inode|No file on disk|✔️|✔️|
|running|full|y|y|n|No inode|No file on disk|✔️|✔️|
|running|full|y|n|n|Inode with scm hash|Inode with scm content|✔️|❌|
|running|full|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|placeholder|y|y/n|n|inode with scm hash|file on disk with scm content|✔️|❌ |
stopped|placeholder|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|y|y/n|n|Inode with scm hash|Inode with scm content|✔️|❌|
|stopped|full|n|y/n|n|No inode|No file on disk|✔️|✔️|

I don’t like this because we are bringing back files with potentially incorrect
contents. Especially for files removed while Eden running, this feels egregious.

2. **We could do the first option, but use empty contents instead of source control contents.**

|File removed while eden is …|Parent is …|Same named file in scm?|Parent is empty (in both inode and on disk)|Tombstone placed?|Inode state after fsck|Fs state after fsck|In sync|No re-appearing files|
|---|---|---|---|---|---|---|---|---|
|running|placeholder|y/n|y/n|y|No inode|No file on disk|✔️|✔️|
|running|full|y|y|n|No inode|No file on disk|✔️|✔️|
|running|full|y|n|n|Materialized inode|Empty file|✔️|❌|
|running|full|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|placeholder|y|y/n|n|Materialized inode|Empty file|✔️|❌ |
|stopped|placeholder|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|y|y/n|n|Inode with scm hash|Empty file|✔️|❌|
|stopped|full|n|y/n|n|No inode|No file on disk|✔️|✔️|

This seems maybe less egregious, but still pretty bad because of the file
removed while eden is running changing after restart case.

3. I think the ideal solution we want is to **keep deleted files deleted.**
That looks like this:

|File removed while eden is …|Parent is …|Same named file in scm?|Parent is empty (in both inode and on disk)|Tombstone placed?|Inode state after fsck|Fs state after fsck|In sync|No re-appearing files|
|---|---|---|---|---|---|---|---|---|
|running|placeholder|y/n|y/n|y|No inode|No file on disk|✔️|✔️|
|running|full|y|y|n|No inode|No file on disk|✔️|✔️|
|running|full|y|n|n|No inode|No file on disk|✔️|✔️|
|running|full|n|y/n|n|No inode|No file on disk|✔️|✔️|
|
stopped|placeholder|y|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|placeholder|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|y|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|n|y/n|n|No inode|No file on disk|✔️|✔️|

This prevents files from re-appearing. Files re-appearing seems like pretty
clearly unexpected behavior to me. But this solution is kinda tricky to
implement. Particularly for the placeholder case. We have to delete the file
through disk. - like `rm` the file in the repo.  Which means more re-entrant
IO. Additionally, this has to happen after Eden starts to ensure a tombstone
gets placed. Currently, FSCK runs before eden starts, so we have queue up some
deletes. And this is kinda messy. Don’t get me wrong I think this is what we
want, but option 4 below is perhaps a good intermediary point.

4. We could **better match ProjectedFS’s behavior** (essentially fix “problem 4”
only and skip “problem 3” for now). ProjectedFS only resurrects files in
placeholder directories. So, we could only resurrect files in placeholder
directories and not full directories. This equates to:


|File removed while eden is …|Parent is …|Same named file in scm?|Parent is empty (in both inode and on disk)|Tombstone placed?|Inode state after fsck|Fs state after fsck|In sync|No re-appearing files|
|---|---|---|---|---|---|---|---|---|
|running|placeholder|y/n|y/n|y|No inode|No file on disk|✔️|✔️|
|running|full|y|y|n|No inode|No file on disk|✔️|✔️|
|running|full|y|n|n|No inode|No file on disk|✔️|✔️|
|running|full|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|placeholder|y|y/n|n|inode with scm hash|file on disk with scm content|✔️|❌ |
|stopped|placeholder|n|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|y|y/n|n|No inode|No file on disk|✔️|✔️|
|stopped|full|n|y/n|n|No inode|No file on disk|✔️|✔️|

This is the easiest solution to implement, as we are purely changing Eden’s
view not ProjectedFS’s. And we get part way to solution 3.

I have already implemented solution 4. And the next steps are to implement a
full solution 3.

To summarize we have four issues in Windows FSCK. #1 FSCK doesn’t recognize the
special-ness of renamed files, #2 Eden generally does bad at renamed files, #3
We echo some bad behavior in ProjectedFS of resurrecting files in placeholder
directories. #4 We over echo that same bad ProjectedFS behavior for full
directories.

#1 and #4, the most critical issues (the ones that make `hg status` wrong),
have fixes on the way. #2 and #3, the others (that make Eden behave in weird
ways), are still pending.
