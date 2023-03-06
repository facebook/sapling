---
sidebar_position: 15
---

# Release notes

See Sapling [VS Code extension changelog](https://github.com/facebook/sapling/blob/94a3dbcb8b03453aed4b6b84d28dd3fdb072ee5e/addons/vscode/CHANGELOG.md) for changes to the sl web UI.

## Feb 28, 2023

This release mainly brings better file move detection as well as various improvements to the `sl pr submit` command

* Added file moves detection for sl diff.
* @discentem added a config option to disable the ReviewStack message in PRs created by `sl pr submit` (https://github.com/facebook/sapling/pull/427). The ReviewStack message is auto disabled for single-commit stack.
* Fixed "mark landed PRs" hook to work when `sl` not in PATH (71d6e67afd)
* Fixed `sl pr submit` to not try to update closed PRs (5a354f7cd6)
* Fixed `sl pr submit` to use "overlapping" PR strategy by default (a05035903a)
* Fix `sl pr submit` crash using non-placeholder issue approach (18a1987638)
* @vegerot added and improved shell completion and prompt (https://github.com/facebook/sapling/pull/369 https://github.com/facebook/sapling/pull/349 https://github.com/facebook/sapling/pull/348).
* @vegerot added support for `sl init --git` on a [non-empty directory](https://github.com/facebook/sapling/pull/463).
* Fixed not being able to launch `sl web` just after building with `make oss`.

## Jan 24, 2023

This release focuses on bug fixes and improvements around the pr and ghstack commands.

* Fixed an issue where sl pr list did not work correctly in combination with chg (https://github.com/facebook/sapling/commit/a1187e8766f4c55bdbf64d369be2ed766641636d).
* @discentem (BK) Updated sl pr pull to throw an appropriate error if no args were specified (https://github.com/facebook/sapling/pull/357).
* Reverted the behavior introduced in the previous release that introduced the practice of using placeholder issues when creating GitHub pull requests. While this approach made it possible to create pull requests in parallel and had desirable guarantees with respect to PR numbers and branch names, it turned out to have a number of downsides that outweighed the benefits, as explained in https://github.com/facebook/sapling/commit/7ce516dfacead64b33fbf664574d169be9ed3b11 .
* @discentem (BK) Fixed a bug where sl clone silently failed for some repositories (https://github.com/facebook/sapling/issues/375) with (https://github.com/facebook/sapling/pull/386).
* Added a PR revset (https://github.com/facebook/sapling/commit/4720a2eff8962b13b44e4255e6c6171eeb084109). Now commands such as sl goto pr123 , sl log -r PR456 should work, even without having to manually download some pull request. Using sl pr pull is still necessary, however, if one wants to get the most recent version of some PR.
* Fix `sl ghstack land` to properly rebase, avoiding spurious "non-fast forward" push errors (https://github.com/facebook/sapling/commit/ebbe7d8d7d71bc144d570f515b5c01da477b4d62), resolving https://github.com/facebook/sapling/issues/333 .

## Dec 22, 2022

This release focuses on correct issues around handling submodules as well as various usability/workflow improvements.

* We made a number of improvements to working in repos with submodules:
    * Rebasing past an update to a submodule no longer adds the submodule change to the bottom of the stack that was rebased: https://github.com/facebook/sapling/commit/2f0f0fdc54aaa000dd6f596ce7e0f57d24bf8695
    * Rebasing a stack that contains a submodule change will preserve the change in the destination if the destination does not change the submodules: https://github.com/facebook/sapling/commit/1f5424d83cc43d439c35255da0975cff452718d5
    * Rebasing a stack with a conflicting submodule change no longer crashes: https://github.com/facebook/sapling/commit/2b94b6f941ab869d61277d1f9c4f5167c68c5258
* Improvements to the sl pr command:
    * You must now specify sl pr submit explicitly: submit is no longer the default subcommand for sl pr, but s can be used as an alias for submit: https://github.com/facebook/sapling/commit/56b5e3e18070207881caeec9b7e0fc96efef4cae
    * The sl pr submit command now supports a --draft flag: https://github.com/facebook/sapling/commit/6e9c3d7a6671839601c8064f0ae5fc2f3e2d361c
    * New sl pr pull subcommand: if you use sl pr submit to create a stack of pull requests, now you can use sl pr pull to import the stack back into your working copy: https://github.com/facebook/sapling/commit/d09d5985c2731df54269169bf8500bd31573baac
    * New sl pr list subcommand that mirrors the functionality of gh pr list: https://github.com/facebook/sapling/commit/8f0a657a00bb24827cb26bc69e345894861a9a38
    * Experimental new command for creating stacks: sl -c github.pr_workflow=single pr submit. See https://github.com/facebook/sapling/issues/302 and https://github.com/facebook/sapling/commit/166e2640d353e67317302f5f998f20c791464402 for details.
    * sl pr submit now appends the stack information to the end of the pull request body instead of prepending it to the top: https://github.com/facebook/sapling/commit/8910d18fe82b791127d62ddfdf631fd474a6f6f3
    * The branch name for a PR created by sl pr submit is now guaranteed to match the PR number: https://github.com/facebook/sapling/commit/e77e67bba710bc6665a4cc9119bc01303ff4509b
* Improvements to commands involving remote names and bookmarks:
    * SCP-style URIs (such as git@github.com:git/git-reference) are now supported for remote names: https://github.com/facebook/sapling/commit/67fa8488e150513d21eedc66b37724d00f2034a9
    * `sl clone --git <URL> --updaterev <branch/commit>` can now be used to clone a specific branch or commit: https://github.com/facebook/sapling/commit/9804c66bb8190a5ab4566c576db391ebe34c2d6b
    * sl rebase -b was reworked for better selecting branching points by avoiding public commits (reverse rebase now works): https://github.com/facebook/sapling/commit/96b767efde6a59dbff31d2808736102b3929067a
    * sl bookmark --remote can be used to list remote branches. Further, sl bookmark --remote-path myfork --remote tags will list tags from remote myfork:
    * https://github.com/facebook/sapling/commit/e8f57d7902c800bec7ded3c567d9e370460df24e
* Removed a dependency on gdbm in the Python code, that was causing crashes for some users: https://github.com/facebook/sapling/commit/cfbb6a256b94c6e755db02658ef1a4312303bee6
