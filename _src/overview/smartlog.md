---
sidebar_position: 20
---

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

Smartlog can be run via `sl smartlog` or by just running `sl`.

```bash
$ sl
o  5abffb82f  Wednesday at 09:39  remote/main
╷
╷ o  824cbba75  13 minutes ago  mary
╷ │  [eden] Support long paths in Windows FSCK
╷ │
╷ │ o  b3c03d03c  Wednesday at 09:39  mary
╷ ├─╯  temp
╷ │
╷ o  19340c083  Wednesday at 09:39  mary
╷ │  [eden] Close Windows file handle during Windows Fsck
╷ │
╷ o  b52192598  Wednesday at 09:39  mary
╭─╯  [eden] Use PathMap for WindowsFsck
│
o  2ac18611a  Wednesday at 05:00  remote/stable
╷
╷ @  0d49848b3  Tuesday at 11:48  mary
╷ │  [edenfs] Recover Overlay from disk/scm for Windows fsck
╷ │
╷ o  97f33204a  Tuesday at 11:48  mary
╷ │  [eden] Remove n^2 path comparisons from Windows fsck
╷ │
╷ o  50dc055b9  Tuesday at 15:40  mary
╭─╯  [eden] Thread EdenConfig down to Windows fsck
│
o  3dfc61ae2  Tuesday at 10:52
╷
╷ o  339f93673  Jul 15 at 11:12  mary
╷ │  debug
╷ │
╷ x  2d4fbea60 [Landed as 04da3d3963ba]  Jul 15 at 11:12  mary
╭─╯  [sl] windows: update Python
│
o  a75ab860a  Jul 15 at 07:59
╷
~
```

In an actual terminal it is color coded, making it easy to read at a glance.

### Interactive GUI Smartlog

An interactive smartlog GUI is available by running `sl web`. This shows similar information to `sl smartlog` while also refreshing automatically and allow you to run commands and drag and drop commits to rebase them.

[See Interactive Smartlog Documentation](../addons/isl.md)
