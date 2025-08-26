/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Core config not loaded in tests. Loaded by both fb and oss.
pub static CONFIG: StaticConfig = static_config!("builtin:production" => r###"
[alias]
metaedit=metaedit --batch
journal=journal --verbose
restack=rebase --restack
restack:doc=automatically restack commits
 ""
 "    When commits are modified by commands like amend and absorb, their"
 "    descendant commits may be left behind as orphans.  Rebase these"
 "    orphaned commits onto the newest versions of their ancestors, making"
 "    the stack linear again."
sl=smartlog -T '{sl}'
ssl=smartlog -T '{ssl}'
top=next --top
top:doc=check out the top commit in the current stack
bottom=prev --bottom
bottom:doc=check out the bottom commit in the current stack

[automv]
similarity=75

[blackbox]
maxfiles=30
maxsize=5242880
logsource=true
track=command, command_alias, command_finish, command_exception,
 commitcloud_sync, exthook, pythonhook, fsmonitor, watchman, merge_resolve,
 profile, hgsql, sqllock, pushrebase, status, metrics, visibility, perftrace

[color]
alerts.critical=bold purple_background
alerts.high=bold red_background
alerts.medium=bold black yellow_background
alerts.low=bold blue_background
alerts.advice=bold black_background
log.changeset=bold blue
sl.active=yellow
sl.amended=brightblack:none
sl.branch=bold
sl.book=green
sl.changed=brightblack:none
sl.current=magenta
sl.desc.current = magenta
sl.diff=bold
sl.draft=brightyellow bold
sl.folded=brightblack:none
sl.hiddenlabel=brightblack:none
sl.hiddennode=brightblack:none
sl.histedited=brightblack:none
sl.highlighted = cyan_background
sl.landed=brightblack:none
sl.undone=brightblack:none
sl.oldbook=bold brightred
sl.public=yellow
sl.rebased=brightblack:none
sl.remote=green
sl.snapshotnode=brightblue:blue
sl.split=brightblack:none
sl.stablecommit = bold green
sl.tasks=bold
sl.backuppending=cyan
sl.backupfail=red
sl.shelvedlabel=bold
ssl.abandoned=brightblack:black+bold
ssl.accepted=brightgreen bold
ssl.committed=cyan
ssl.landing=green
ssl.review=#f7923b:color214:brightyellow bold
ssl.revision=#f21395:color199:brightmagenta bold
ssl.unpublished=#8d949e:color248:none
ssl.landfailed=red
ssl.finalreview=#8036cc:color92:magenta
ssl.unsync=brightred bold
ssl.syncdraft = #8d949e:color248:none
ssl.signal_okay=green
ssl.signal_in_progress=cyan
ssl.signal_warning=yellow
ssl.signal_failed=red
sb.active=green
doctor.treatment=yellow

[connectionpool]
lifetime=300

[debugnetwork]
speed-test-download-size=10M
speed-test-upload-size=2M

[diff]
git=True
nobinary=True

[directaccess]
loadsafter=tweakdefaults

[experimental]
bundle2-exp=True
changegroup3=True
crecord=True
fsmonitor.transaction_notify=True
graphstyle.grandparent=|
histedit.autoverb=True
metalog=True
narrow-heads=True
new-clone-path=True
rebase.multidest=True
samplestatus=3
verifyhiddencache=False
worddiff=True
mmapindexthreshold=1
numworkersremover=1
nativecheckout=True

[extensions]
absorb=
amend=
automv=
blackbox=
chistedit=
clindex=
conflictinfo=
debugnetwork=
dialect=
directaccess=
dirsync=
errorredirect=!
fastpartialmatch=
fbhistedit=
fsmonitor=
ghstack=
githelp=
gitrevset=!
hgsubversion=!
histedit=
journal=
logginghelper=
morestatus=
myparent=
obsshelve=
patchrmdir=!
progressfile=
pushrebase=!
rage=!
rebase=
remotefilelog=
shelve=
smartlog=
sparse=
strip=
traceprof=
treemanifest=
undo=

# extorder is put at the end intentionally
extorder=

[extorder]
fsmonitor=sparse
journal=eden

[format]
aggressivemergedeltas=True
generaldelta=True
manifestcachesize=10
maxchainlen=30000
use-zstore-commit-data-revlog-fallback=true
use-zstore-commit-data=False
noloosefile=True
userustmutablestore=True
use-symlink-atomic-write=False

[fsmonitor]
mode=on
timeout=600
track-ignore-files=False
warn-fresh-instance=True

[histgrep]
allowfullrepogrep=False

[merge]
checkignored=ignore
checkunknown=ignore
on-failure=continue
printcandidatecommmits=True

[merge-tools]
editmergeps.args=$output
editmergeps.check=changed
editmergeps.premerge=keep

[mutation]
enabled=true
record=true

[phases]
publish=False

[remotefilelog]
cachelimit=20GB
useruststore=True
manifestlimit=4GB
http=True
retryprefetch=True
fetchpacks=True
getpackversion=2

[remotenames]
autopullpattern=re:^remote/[A-Za-z0-9._/-]+$
cachedistance=False
disallowedhint=please don't specify 'remote/' prefix in remote bookmark's name
disallowedto=^remote/
hoist=remote
precachecurrent=False
precachedistance=False
pushanonheads=False
pushrev=.
rename.default=remote
resolvenodes=False
selectivepulldefault=master

[server]
preferuncompressed=True
uncompressed=True

[sigtrace]
interval=30

[templatealias]
# NOTE: There's a very specific spacing scheme at play:
#   - One space between items within a section
#   - Two spaces between sections
sl_hash_minlen=10
sl_phase_label="{ifeq(phase, 'public', 'sl.public', 'sl.draft')}"
sl_node="{label(sl_phase_label, shortest(node, sl_hash_minlen))}"
sl_node_debug="{label(sl_phase_label, '{node}')}"
sl_phase_debug="{ifeq(phase, 'draft', '', '({phase})')}"

sl_undonode="{label('sl.oldbook', '{shortest(node, sl_hash_minlen)}')}"
sl_donode="{label('sl.book', '{shortest(node, sl_hash_minlen)}')}"
sl_some="{label('sl.draft', '{shortest(node, sl_hash_minlen)}')}"
undo_node_info="{if(undonecommits(UNDOINDEX), sl_undonode, if(donecommits(UNDOINDEX), if(revset('{node} and olddraft(0)'), sl_some, sl_donode), sl_node))}"

sl_user="{label('sl.user', author|emailuser)}"
sl_active="{if(activebookmark, label('sl.active', '{activebookmark}*'))}"
sl_labeled_bm="{label('sl.book', bookmark)}"
sl_nonactive="{ifeq(bookmark, active, '', '{sl_labeled_bm} ')}"
sl_others="{strip(bookmarks % '{sl_nonactive}')}"
sl_remote="{label('sl.remote', remotebookmarks)}"
sl_books="{separate(' ', sl_active, sl_others, sl_remote)}"
sl_cloud_books="{separate(' ', sl_active, sl_others)}"

sl_oldbm="{label('sl.book', oldbookmarks(UNDOINDEX))}"
sl_leaving="{label('sl.oldbook', removedbookmarks(UNDOINDEX))}"
sl_bookchanges="{separate(' ', sl_leaving, sl_oldbm)}"

sl_signal_okay="✓"
sl_signal_failed="✗"
sl_signal_warning="‼"
sl_signal_in_progress="⋯"

github_sl_difflink="{
  if(github_pull_request_url,
    hyperlink(github_pull_request_url, '#{github_pull_request_number}'),
    if(sapling_pr_follower, label('ssl.unpublished', 'follower'))
  )}"
phab_sl_difflink="{hyperlink(separate('', 'https://www.internalfb.com/diff/', phabdiff, '/'), phabdiff)}"
sl_difflink="{if(github_repo, github_sl_difflink, phab_sl_difflink)}"

github_sl_diffsignal="{case(github_pull_request_status_check_rollup, 'SUCCESS', sl_signal_okay, 'PENDING', sl_signal_in_progress, 'FAILURE', sl_signal_failed)}"
github_sl_diffsignallabel="{case(github_pull_request_status_check_rollup, 'SUCCESS', 'ssl.signal_okay', 'PENDING', 'ssl.signal_in_progress', 'FAILURE', 'ssl.signal_failed')}"
phab_sl_diffsignal="{case(phabsignalstatus, 'TEST_FINISHED_WITH_NO_FAILURES', sl_signal_okay, 'LANDED_WITH_NO_FAILURES', sl_signal_okay, 'OLD_DIFF_WITH_PASSING_PG_SIGNALS', sl_signal_okay, 'NOT_STARTED', sl_signal_in_progress, 'PREPARE_TO_START', sl_signal_in_progress, 'TEST_DEFERRED', sl_signal_in_progress, 'TEST_IN_PROGRESS_WITH_NO_FAILURES', sl_signal_in_progress, 'LAND_SCHEDULED', sl_signal_in_progress, 'LAND_ENQUEUED', sl_signal_in_progress, 'LAND_IN_PROGRESS_WITH_NO_FAILURES', sl_signal_in_progress, 'TEST_IN_PROGRESS_WITH_WARNINGS', sl_signal_warning, 'TEST_FINISHED_WITH_WARNINGS', sl_signal_warning, 'LAND_IN_PROGRESS_WITH_WARNINGS', sl_signal_warning, 'LANDED_WITH_WARNINGS', sl_signal_warning, 'LAND_ON_HOLD', sl_signal_warning, 'LAND_CANCELLED', sl_signal_warning, 'TEST_IN_PROGRESS_WITH_FAILURES', sl_signal_failed, 'TEST_FINISHED_WITH_FAILURES', sl_signal_failed, 'LAND_FAILED', sl_signal_failed, 'LAND_IN_PROGRESS_WITH_FAILURES', sl_signal_failed, 'LANDED_WITH_FAILURES', sl_signal_failed, 'OLD_DIFF_WITH_FAILED_PG_SIGNALS', sl_signal_okay)}"
phab_sl_diffsignallabel="{case(phabsignalstatus, 'TEST_FINISHED_WITH_NO_FAILURES', 'ssl.signal_okay', 'LANDED_WITH_NO_FAILURES', 'ssl.signal_okay', 'OLD_DIFF_WITH_PASSING_PG_SIGNALS', 'ssl.signal_okay', 'NOT_STARTED', 'ssl.signal_in_progress', 'PREPARE_TO_START', 'ssl.signal_in_progress', 'TEST_DEFERRED', 'ssl.signal_in_progress', 'TEST_IN_PROGRESS_WITH_NO_FAILURES', 'ssl.signal_in_progress', 'LAND_SCHEDULED', 'ssl.signal_in_progress', 'LAND_ENQUEUED', 'ssl.signal_in_progress', 'LAND_IN_PROGRESS_WITH_NO_FAILURES', 'ssl.signal_in_progress', 'TEST_IN_PROGRESS_WITH_WARNINGS', 'ssl.signal_warning', 'TEST_FINISHED_WITH_WARNINGS', 'ssl.signal_warning', 'LAND_IN_PROGRESS_WITH_WARNINGS', 'ssl.signal_warning', 'LANDED_WITH_WARNINGS', 'ssl.signal_warning', 'LAND_ON_HOLD', 'ssl.signal_warning', 'LAND_CANCELLED', 'ssl.signal_warning', 'TEST_IN_PROGRESS_WITH_FAILURES', 'ssl.signal_failed', 'TEST_FINISHED_WITH_FAILURES', 'ssl.signal_failed', 'LAND_FAILED', 'ssl.signal_failed', 'LAND_IN_PROGRESS_WITH_FAILURES', 'ssl.signal_failed', 'LANDED_WITH_FAILURES', 'ssl.signal_failed', 'OLD_DIFF_WITH_FAILED_PG_SIGNALS', 'ssl.signal_okay')}"
# sl_diffsignal is a glyph while sl_diffsignallabel is the style.
sl_diffsignal="{if(github_repo, github_sl_diffsignal, phab_sl_diffsignal)}"
sl_diffsignallabel="{if(github_repo, github_sl_diffsignallabel, phab_sl_diffsignallabel)}"

# sl_diffstatus is the text while sl_difflabel is the style.
sl_diffstatus="{if(github_repo, github_sl_diffstatus, phab_sl_diffstatus)}"
sl_difflabel="{if(github_repo, github_sl_difflabel, phab_sl_difflabel)}"
alerts="\n     Ongoing issue\n 🔥  {label(severity_color, ' {severity} ')} {hyperlink(url, title)}\n     {description}\n\n"

# When presenting the PR state to the user, if it is Closed/Merged, we use
# that as the state; otherwise (if it is Open/Unknown), we consider the review
# decision to be the PR state.
github_pr_state="{case(github_pull_request_state,
    'CLOSED', 'CLOSED',
    'MERGED', 'MERGED',
    if(github_pull_request_review_decision, github_pull_request_review_decision, 'NO_DECISION')
    )}"
github_sl_diffstatus="{case(github_pr_state,
    'CLOSED', 'Closed',
    'MERGED', 'Merged',
    'APPROVED', 'Approved',
    'CHANGES_REQUESTED', 'Changes Requested',
    'REVIEW_REQUIRED', 'Review Required',
    'NO_DECISION', 'Unreviewed'
    )}"
# At least for ghstack, PRs are never merged directly because of the branch
# tricks it does, so Merged often means Closed, so we use the same style for
# both. We could consider checking for the "Merged" tag to distinguish between
# "closed but merged" and ordinary "closed", which is more like Abandoned in
# Phabricator.
github_sl_difflabel="{case(github_pr_state,
    'CLOSED', 'ssl.committed',
    'MERGED', 'ssl.committed',
    'APPROVED', 'ssl.accepted',
    'CHANGES_REQUESTED', 'ssl.revision',
    'REVIEW_REQUIRED', 'ssl.review',
    'NO_DECISION', 'ssl.unpublished',
    'sl.diff'
    )}"

phab_sl_diffstatus="{phabstatus}{case(phabstatus, 'Waiting For Author', '*', 'Needs Revision', '*')}"
phab_sl_difflabel="{case(phabstatus,
 'Landing', 'ssl.landing',
 'Accepted', 'ssl.accepted',
 'Waiting For Author', 'ssl.revision',
 'Needs Revision', 'ssl.revision',
 'Changes Planned', 'ssl.revision',
 'Committed', 'ssl.committed',
 'Needs Review', 'ssl.review',
 'Abandoned', 'ssl.abandoned',
 'Unpublished', 'ssl.unpublished',
 'Committing', 'ssl.landing',
 'Recently Failed to Land', 'ssl.landfailed',
 'Needs Final Review', 'ssl.finalreview',
 'sl.diff'
 )}"

sl_date = "{label('sl.date', '{smartdate(sl_date_timestamp, sl_date_threshold, age(sl_date_timestamp), simpledate(sl_date_timestamp, sl_date_timezone))}')}"
sl_date_timestamp = date
sl_date_threshold = 5400
sl_diff = "{label('sl.diff', sl_difflink)}"
sl_task = "{label('sl.tasks', 'T{task}')}"
sl_tasks = "{join(tasks % '{sl_task}', ' ')}"
sl_backupstatus = "{ifcontains(rev, revset('notbackedup()'), if(backingup, label('sl.backuppending', '(Backing up)'), ifcontains(rev, revset('{node} and age(\'<10m\')'), label('sl.backuppending', '(Backup pending)'), label('sl.backupfail', '(Not backed up)'))))}"
sl_backup="{if(enabled('commitcloud'),sl_backupstatus)}"
sl_landed = "{if(singlepublicsuccessor, label('sl.landed', '(Landed as {shortest(singlepublicsuccessor, sl_hash_minlen)})'))}"
sl_amended = "{if(amendsuccessors, label('sl.amended', '(Amended as {join(amendsuccessors % \'{shortest(amendsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_rebased = "{if(rebasesuccessors, label('sl.rebased', '(Rebased as {join(rebasesuccessors % \'{shortest(rebasesuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_split = "{if(splitsuccessors, label('sl.split', '(Split into {join(splitsuccessors % \'{shortest(splitsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_folded = "{if(foldsuccessors, label('sl.folded', '(Folded into {join(foldsuccessors % \'{shortest(foldsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_histedited = "{if(histeditsuccessors, label('sl.histedited', '(Histedited as {join(histeditsuccessors % \'{shortest(histeditsuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_undone = "{if(undosuccessors, label('sl.undone', '(Undone to {join(undosuccessors % \'{shortest(undosuccessor, sl_hash_minlen)}\', ', ')})'))}"
sl_mutation_names = dict(amend="Amended as", rebase="Rebased to", split="Split into", fold="Folded into", histedit="Histedited to", land="Landed as", pushrebase="Landed as")
sl_mutations = "{join(mutations % '[{get(sl_mutation_names, operation, \'Rewritten into\')} {join(successors % \'{node|short}\', \', \')}]', ' ')}"
sl_hidden = "{ifcontains(rev, revset('hidden()'), label('sl.hiddenlabel', '(hidden)'))}"
sl_label="{ifeq(highlighted_node, node, 'sl.highlighted', ifeq(graphnode, '@', 'sl.current', ifeq(graphnode, 'x', 'sl.hiddennode', ifeq(graphnode, 's', 'sl.snapshotnode'))))}"
sl_desclabel = "{ifeq(graphnode, '@', 'sl.desc.current', ifeq(graphnode, 'x', 'sl.desc.hiddennode', ifeq(phase, 'public', 'sl.desc.public', 'sl.desc')))}"
sl_shelved = "{if(shelveenabled, ifcontains(rev, revset('shelved()'), label('sl.shelvedlabel', '(shelved)')), '')}"
ssl_unsync = "{label('ssl.unsync', ifeq(syncstatus, 'unsync', '(local changes)'))}"
ssl_diffversion = if(diffversion,ifcontains("+",diffversion,label("ssl.unsync","{diffversion}"),ifcontains(".",diffversion,label("ssl.syncdraft","{diffversion}"),label("ssl.sync","{diffversion}"))))
ssl_unsync_or_diffversion = ifeq(ssl_show_diffversion,"true",ssl_diffversion,ssl_unsync)
ssl_show_diffversion = ifeq(verbosity, "verbose", "true")
sl_stablecommit = "{label('sl.stablecommit', smallcommitmeta('arcpull_stable'))}"

sl_node_info = "{separate(' ', sl_node, sl_mutations, sl_backup, sl_shelved)}"
sl_node_info_debug = "{separate(' ', sl_node_debug, sl_mutations, sl_hidden, sl_backup, sl_shelved, sl_phase_debug)}"
sl_diff_super = "{ifeq(graphnode, 's', '', if(sl_diff, separate(' ', label(sl_difflabel, separate(' ', sl_difflink, sl_diffstatus)), label(sl_diffsignallabel, sl_diffsignal), ssl_unsync_or_diffversion)))}"
sl_diff_colors = "{if(sl_diff, label(sl_difflabel, '{phabdiff}'))}"

sl_header_normal = "{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_super = "{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff_super, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_colors = "{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_user, sl_diff_colors, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_debug = "{separate('  ', sl_userdefined_prefix, sl_node_info_debug, sl_date, sl_user, sl_diff, sl_tasks, sl_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_header_short = "{separate('  ', sl_userdefined_prefix, sl_node_info, sl_date, sl_books, sl_stablecommit, sl_userdefined_suffix)}"

# The 2 is to ensure that there is room for one space padding between the graph and description, and one between the end of the line and the right of the terminal.
sl_desc = "{label(sl_desclabel, truncatelonglines(desc|firstline, termwidth - graphwidth - 2, ellipsis))}"
ellipsis = '…'

# Whether to show with the short header template (no author, diff, task or description).
sl_use_short_header = "{ifeq(verbosity, 'verbose', '', ifeq(graphnode, '@', '', ifeq(phase, 'public', ifeq('{username|email}', '{author|email}', '', 'true'))))}"

# A list of changed files (truncated to a limit)
sl_file_count = 0
sl_file_info = "{ifeq(phase, "public", "", ifgt(sl_file_count, 0, truncate("{files%'- {file}\n'}\n", sl_file_count, "  (...and {count(files)-sl_file_count+1} more file{ifeq(count(files), sl_file_count, "", "s")}...)\n\n"), ""))}"

# A path-prefix-based summary of the files that have changed.
sl_file_change_summary = '{ifeq(sl_show_file_change_summary,"true",ifeq(phase, "draft", "{label(\"status.modified\", pathsummary(file_mods) % \" M {path}\n\")}{label(\"status.added\", pathsummary(file_adds, 3) % \" A {path}\n\")}{label(\"status.removed\", pathsummary(file_dels, 3) % \" R {path}\n\")}\n"))}'

# Whether to show the file change summary. Set by `--commit-info` of `sl`.
sl_show_file_change_summary='false'

# Additional lines to be rendered after the commit summary
sl_extra = "{sl_file_info}{ifeq(sl_file_count,0,sl_file_change_summary)}"

# Normal smartlog
sl = "{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_normal, sl_desc, '\n')))}{sl_extra}"

# Super-smartlog.  Includes phabricator information.
ssl = "{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_super, sl_desc, '\n')))}{sl_extra}"

# Like super-smartlog, but only colorizes the diff number.
csl = "{label(sl_label, if(sl_use_short_header, sl_header_short, separate('\n', sl_header_colors, sl_desc, '\n')))}{sl_extra}"

# Smartlog with debug information.  Used by hg rage.
sl_debug = "{label(sl_label, separate('\n', sl_header_debug, sl_desc, '\n'))}"

undo_newwp = "{if(oldworkingcopyparent(UNDOINDEX), '(working copy will move here)')}"
undopreview = "{separate('\n', separate('  ', undo_node_info, undo_newwp, sl_user, sl_diff, sl_tasks, sl_bookchanges), '{sl_desc}', '\n')}"

sb_date = "{date(date, '%x')}"
sb_item = "{sb_date} {bookmarks}\n           {desc|firstline}\n"
sb_active = "{label('sb.active', sb_item)}"
sb = "{if(activebookmark, sb_active, sb_item)}"

# Commit Cloud Templates
sl_cloud_node = "{label(sl_phase_label, ifeq(phase, 'draft', hyperlink(separate('', 'https://www.internalfb.com/phabricator/commit/FBS/', node), truncatelonglines(node, sl_hash_minlen)), truncatelonglines(node, sl_hash_minlen)))}"
sl_cloud_header_super = "{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_user, sl_diff_super, sl_tasks, sl_cloud_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_cloud_header_normal = "{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_user, sl_diff, sl_tasks, sl_cloud_books, sl_stablecommit, sl_userdefined_suffix)}"
sl_cloud_header_short = "{separate('  ', sl_userdefined_prefix, sl_cloud_node, sl_date, sl_cloud_books, sl_userdefined_suffix)}"
sl_cloud = "{label(sl_label, if(sl_use_short_header, sl_cloud_header_short, separate('\n', sl_cloud_header_normal, sl_desc, '\n')))}"
ssl_cloud = "{label(sl_label, if(sl_use_short_header, sl_cloud_header_short, separate('\n', sl_cloud_header_super, sl_desc, '\n')))}"

jf_submit_template = '\{
  "node": {node|json},
  "date": {date|json},
  "desc": {desc|utf8|json},
  "diff": {diff()|json},
  "files": {files|json},
  "user": {author|utf8|json},
  "parents": ["{p1node}" {ifeq(p2node, "0000000000000000000000000000000000000000","",", \"{p2node}\"")}],
  "publicbase": {singlepublicbase|json},
  "publicbase_svnrev": {revset('last(::%d - not public())', rev) % '{svnrev}'|json},
  "parentrevision": {myparentdiff|json},
  "phase": {phase|json}
  },\n'

[revsetalias]
sb(n)=first(sort(bookmark(), -rev), n)
sba=sort(bookmark(), -rev)
top=top()
bottom=bottom()
bot=bottom()
next=next()
prev=previous
previous=previous()
base=last(public() & ::.)
obsrelated(x)=mutrelated(x)
focusedsmartlog(x)=focusedbranch(x) + draftbranch(x)^ + present(master)

[treestate]
automigrate=True

[treemanifest]
sendtrees=True
treeonly=True
http=True
usecunionstore=False
rustmanifest=True
pullprefetchrevs=master
ondemandfetch=True
prefetchdraftparents=False
useruststore=True

[tweakdefaults]
showupdated=True
defaultdest=remote/master
tagsmessage=NOTE: new tags are disabled in this repository
singlecolonwarn=True

[ui]
enableincomingoutgoing=False
hyperlink=True
interface=curses
logmeasuredtimes=True
merge:interactive=editmerge
mergemarkers=detailed
rollback=False
suggesthgprev=True
threaded=False
merge=internal:merge
origbackuppath = @DOTDIR@/origbackups

[progress]
renderer=simple

[verify]
skipmanifests=True

[visibility]
enabled=true

[worker]
rustworkers=True
numcpus=4

[clindex]
nodemap=True
verify=False

[committemplate]
changeset = {if(desc, desc, emptymsg)}\n
    HG: Enter commit message.  Lines beginning with 'HG:' are removed.
    HG: Leave message empty to abort commit.
    HG: --
    HG: user: {author}\n{ifgt(parents|count, 1,
   "HG: merging:\n{parents % 'HG:   {node|short}: {desc|firstline}\n'}")
   }{if(currentbookmark,
   "HG: bookmark '{currentbookmark}'\n")}{
    filechanges}{
    if(advice, advice, defaultadvice)}

defaultadvice = HG: --
    HG: Consider onboarding Jellyfish in this repo to speed up your workflow.
    HG: Learn how at https://fburl.com/jf-onboard\n

defaulttitle=<Replace this line with a title. Use 1 line only, 67 chars or less>

filechanges={ifgt(parents|count, 1, filechangesmerge,
 ifgt(files|count, filechangethreshold, filechangesplain, filechangesdetailed))}
filechangesmerge=
filechangethreshold=100
filechangesplain = {
    file_adds % "HG: added {file}\n"}{
    file_mods % "HG: changed {file}\n"}{
    file_dels % "HG: removed {file}\n"}{
    if(files, "", "HG: no files changed\n")}
filechangesdetailed={ifeq(verbosity,"verbose",diff()|hgprefix,stat("status")|hgprefix)}\n

[copytrace]
dagcopytrace=True
hint-with-commit=True

[profiling:background]
enabled=1
format=text
freq=1
minelapsed=20
output=blackbox
sort=inlinetime
statformat=hotpath
type=stat

[smartlog]
repos=remote/
indentnonpublic=True

[rebase]
experimental.inmemory.nomergedriver=False
experimental.inmemory=True

[packs]
maxpacksize=8GB
maxpackfilecount=1024
maxdatapendingbytes=4GB
maxdatabytes=50GB
maxhistorybytes=20GB

[amend]
autorestack=no-conflict

[absorb]
amendflag=stack

[histedit]
defaultrev=limit(only(.) & draft(), 50)
dropmissing=True
linelen=1000

[morestatus]
show=True

[clone]
default-destination-dir=$HOME
nativecheckout=True
nativepull=True

[scale]
largeworkingcopy=True

[deprecate]
rev-option=true
clone-include-option=true
clone-exclude-option=true

[indexedlog]
data.max-bytes-per-log=7G

[pull]
httpbookmarks=True
httphashprefix=True
master-fastpath=True

[exchange]
httpcommitlookup=True

[scmstore]
lfsptrwrites=True
auxindexedlog=True

[pager]
pager=internal:streampager

[config]
use-rust=true

[status]
use-rust=true

[commitcloud]
supported-url-regex = ^.*\.(facebook|tfbnw|mononoke)\..*$
"###);
