---
sidebar_position: 20
---
import {SLCommand} from '@site/elements'

# Smartlog

Forming a mental picture of a repository is one of the largest hurdles for users learning distributed version control. A poor mental model leads to people not understanding what commands actually do, not understanding how to recover from mistakes, not being able to use advanced features, and generally leads to people copy/pasting commands they’ve memorized and then recloning when things go awry.

Instead of requiring users to piece together the state of their repo via `log`, `branch`, etc, we built Smartlog and made it the centerpiece of Sapling’s user experience. As such, `sl smartlog` is one of the most important commands in Sapling as it lets you see the state of your local repo clearly and concisely, without having to learn multiple commands or maintain a complex mental model of the repository.

It shows you:

- Your not-yet-pushed commits.
- The location of main, and other important branches (configurable).
- The graph relationship between these commits.
- Where you are (`@`).
- Which commits are old and have been landed, rebased, amended, etc. See the ‘x’ commit, with the "Landed as YYY" message.
- Important details for each commit: short hash, date, author, local and remote bookmarks, and title. It can be configured to show other information from inside the commit as well, such as task number or pull request number.

Smartlog provides you with a succinct view of your work by hiding all commits that aren’t relevant to you. In the example below, the dashed line on the left represents the main branch and elides thousands of commits to show you just the ones relevant to you.

Smartlog can be displayed via `sl smartlog` or by just running `sl`.

<pre>
<span class="shell-prompt">&gt;</span> <span class="shell-command">sl</span><br />
o  <span class="sl-public">5abffb82f</span>  Wednesday at 09:39  <span class="sl-bookmark">remote/main</span><br />
╷<br />
╷ o  <span class="sl-draft">824cbba75</span>  13 minutes ago  mary<br />
╷ │  [eden] Support long paths in Windows FSCK<br />
╷ │<br />
╷ │ o  <span class="sl-draft">b3c03d03c</span>  Wednesday at 09:39  mary<br />
╷ ├─╯  temp<br />
╷ │<br />
╷ o  <span class="sl-draft">19340c083</span>  Wednesday at 09:39  mary<br />
╷ │  [eden] Close Windows file handle during Windows Fsck<br />
╷ │<br />
╷ o  <span class="sl-draft">b52192598</span>  Wednesday at 09:39  mary  <span class="sl-diff">#12</span><br />
╭─╯  [eden] Use PathMap for WindowsFsck<br />
│<br />
o  <span class="sl-public">2ac18611a</span>  Wednesday at 05:00  <span class="sl-bookmark">remote/stable</span><br />
╷<br />
╷ @  <span class="sl-draft">0d49848b3</span>  Tuesday at 11:48  mary<br />
╷ │  <span class="sl-current">[edenfs] Recover Overlay from disk/scm for Windows fsck</span><br />
╷ │<br />
╷ o  <span class="sl-draft">97f33204a</span>  Tuesday at 11:48  mary<br />
╷ │  [eden] Remove n^2 path comparisons from Windows fsck<br />
╷ │<br />
╷ o  <span class="sl-draft">50dc055b9</span>  Tuesday at 15:40  mary<br />
╭─╯  [eden] Thread EdenConfig down to Windows fsck<br />
│<br />
o  <span class="sl-public">3dfc61ae2</span>  Tuesday at 10:52<br />
╷<br />
╷ o  <span class="sl-draft">339f93673</span>  Jul 15 at 11:12  mary<br />
╷ │  debug<br />
╷ │<br />
╷ x  2d4fbea60 [Landed as 04da3d3963ba]  Jul 15 at 11:12  mary  <span class="sl-diff">#11</span><br />
╭─╯  <span class="sl-obsolete">[sl] windows: update Python</span><br />
│<br />
o  <span class="sl-public">a75ab860a</span>  Jul 15 at 07:59<br />
╷<br />
~<br />
</pre>

### Super Smartlog

Sapling can also fetch information about the repository from external sources, such as checking GitHub to know if a pull request has passed automated tests and been reviewed. Since this extra information requires waiting a few seconds for network requests, we have a separate <SLCommand name="ssl" /> alias for this:

<pre>
<span class="shell-prompt">&gt;</span> <span class="shell-command">sl ssl</span><br />
o  <span class="sl-public">bc3bbba5d</span>  Yesterday at 12:23  <span class="sl-bookmark">remote/main</span><br />
╷<br/>
╷ @  <span class="sl-draft">c7ed677ea</span>  Today at 11:17  jane  <span class="sl-review-unreviewed">#269 Unreviewed</span> <span class="sl-signal-failed">✗</span><br />
╷ │  <span class="sl-current">[docs] make examples consistent</span><br />
╷ │<br />
╷ o  <span class="sl-draft">9f15ade1c</span>  Today at 10:09  jane  <span class="sl-review-unreviewed">#267 Unreviewed</span> <span class="sl-signal-okay">✓</span><br />
╷ │  [docs] syntax-highlighting for smartlogs<br />
╷ │<br />
╷ o  <span class="sl-draft">44df3afe6</span>  Yesterday at 14:07  jane  <span class="sl-review-approved">#264 Approved</span> <span class="sl-signal-okay">✓</span><br />
╭─╯  [docs] add sl-shell-example syntax-highlighting language<br />
│<br />
o  <span class="sl-public">ecf6ca5e1</span>  Yesterday at 12:23<br />
│<br />
~<br />
</pre>

### Interactive GUI smartlog

An interactive smartlog GUI is available by running `sl web`. This shows similar information to `sl smartlog` while also refreshing automatically, and allows you to run commands or drag and drop commits to rebase them.

[See Interactive Smartlog Documentation](../addons/isl.md)
