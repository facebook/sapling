---
sidebar_position: 3
---

# Git support modes

Sapling supports Git in 2 modes. Here is a quick comparison:

|      | `.git` mode | `.sl` mode |
|------|-------------|------------|
| Repo created by | `git clone` or `git init` | `sl clone` or `sl init` |
| Repo directory | `.git` | `.sl` |
| `git` commands | ✅ Supported | ⚠️ Not supported |
| [Git LFS](https://git-lfs.com/) | Partially supported [^1] | ⚠️ Not supported |
| Submodules | Work-In-Progress | [Supported without EdenFS](/docs/git/submodule.md) |
| [ISL](/docs/addons/isl.md) (graphic interface) | ✅ Supported | ✅ Supported |
| EdenFS (virtual filesystem) | ⚠️ Not supported | ✅ Supported (experimental [^2]) |
| Git network protocol (Git or Mononoke server) | ✅ Supported | ✅ Supported |
| Sapling network protocol (Mononoke server) | ⚠️ Not supported | ✅ Supported (experimental [^2]) |

[^1]: Sapling can write Git LFS files to disk on `goto`. However, if you need to make changes or create new LFS files, or view their diffs, you need to use `git` commands.
[^2]: Source code to support those exists on GitHub. However, EdenFS, Mononoke, and Sapling with EdenFS support are not yet part of the public GitHub releases yet.


## `.git` mode

In this mode, Sapling tries to be compatible with `.git/` file formats so you can run `git` commands. For features not natively supported by Git like [mutation](/docs/dev/internals/visibility-and-mutation.md#commit-mutation), Sapling will store them in the `.git/sl` directory.

There are some caveats using the `.git` mode:

- Mixing `sl` and `git` commands might not work in all cases. For example:
  - If your `sl rebase` is interrupted, use `sl rebase --continue` to continue, you cannot use `git rebase --continue`.
  - Similarity, use `git rebase --continue` for an interrupted `git rebase`.
  - If you run `sl add` to mark a file as tracked, use `sl status` and `sl commit` to commit the change. `sl add` might not affect `git status` or `git commit`.
- `sl` might put the repo in a ["detached head"](https://git-scm.com/docs/git-checkout#_detached_head) state. This is okay if you only run `sl` commands. However, if you use `git` commands, be sure to run `git branch some_name` to create a branch before `git checkout` away to avoid losing commits.
- LFS [^1] or other `.gitattributes` features are partially supported. `sl goto` runs `git checkout` under the hood. Other `sl` commands like `commit` or `diff` do not respect `.gitattributes`. You can use `git` instead.
- `sl` does not have local tags. You can use `origin/tags/v1.0` to refer to the `v1.0` tag stored on the `origin` server.
- Currently, sub-modules are not fully supported.
- Currently, ISL might not detect repo changes as quickly or automatically as other modes. Use the refresh button to recheck manually if necessary.


## `.sl` mode

In this mode, Sapling can utilize more scalability features, although some are not yet built in the public release. For example:

For the working copy implementation, Sapling can use its own implementations:
- Physical filesystem: together with [watchman](https://facebook.github.io/watchman/), `add` or `remove` operations can be O(changed files) instead of O(total files). There is no need to re-write a potentially large file like [`.git/index`](https://git-scm.com/docs/gitformat-index).
- Virtual filesystem (EdenFS): With `eden`, and `sl` built with Thrift support, you can run `eden clone <sl_git_working_copy> <new_working_copy>` to get a virtual working copy which has much better `goto` performance.

For server protocols, Sapling can use dedicated lazy commit graph protocols so `clone` and `pull` are roughly O(merges) both in time and space usage.

For local storage, Sapling can use its [own structure](/docs/dev/internals/indexedlog.md) and [compression](/docs/dev/internals/zstdelta.md) so the file count is bounded and there is no need to repack. Note: This is not fully implemented for the git format yet but is the direction we'd like to go.

