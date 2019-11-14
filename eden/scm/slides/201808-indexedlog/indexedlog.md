
# Indexed Log

<!-- animation: true -->

---

# Agenda

- Problems
- Indexed Log

---

# Revlog

- 2 revlog files per file to track
- Delta-ed

Current usage

- Client: Changelog
- Server: Everything (Changelog, manifest, filelog)
  - hgsql enforced

---

# Revlog

The single data structure powering most of the vanilla Mercurial.

```bob,scale=0.8
         .i                 |                .d
+------------------+        |        +-------------------+
| rev 0  metadata  | -- points to -> | rev 0  full text  |
+------------------+        |        +--------------+----+
| rev 1  metadata  | -- points to -> | rev 1  delta |
+------------------+        |        +--------------+-+
| rev 2  metadata  | -- points to -> | rev 2  delta   |
+------------------+        |        +----------------+
                            |
|<--- 64 bytes --->|        |        |<- variant sized ->|

```

---

# Revlog

- $O(1)$ lookup by *Revision Numbers*
- $O(1)$ insertion
- Also has *SHA1 Hashes* for integrity check

Problems

- $O(N)$ lookup by *SHA1 Hash* (first time without index)
- Too many inodes (Filelogs) 
- Sparse is hard (Topological sorted revision number)

---

# Loose files and Pack files

The two formats powering Git.

Git does not have *Revision Numbers*.

Remotefilelog is similar.

---

# Loose file

- One file per file *revision*
- No Deltas
- $O(\log N)$ lookup by *SHA1 Hash*
  - By kernel (filesystem)

Problems

- *Extremely* space inefficient
- Way too many inodes

---

# Pack file

- 1 pack file for a range of file revisions 
- Delta-ed

---

# Pack file

```bob
    .idx                   |    .pack
                           |
Level1      Level2         |    Similar to
1st byte    Sorted SHA1s   |    revlog.d
+----+     +------+        |   +-----------+
| 00 | --> | 0000 | ---------> | full text |
+----+     | 0002 | ---.   |   +---------+-+
| 01 |     | ...  |     \ .--> | delta   |
+----+     | 00ff | -.   X |   +--------++
| .  |     +------+   \ / `--> | delta  |
| .  |                 X   |   +--------+----+ 
| .  |     +------+   / \.---> | full text   | 
+----+     | ff01 | -'  /\ |   +-------+-----+ 
| ff | --> | ff02 | ---'  '--> | delta |
+----+     +------+        |   +-------+
```

---

# Pack file

- $O(\log N)$ lookup
- $O(\frac{N}{256})$ same-file insertion. $O(1)$ creating new file insertion.

Problems
- Too many ($M$) pack files degrades performance
  - $O(M \log \frac{N}{M})$ lookup
- And space, if pack files are self-contained
  - Delta-chain become less efficient
- Must do `repack` to maintain performance
  - `repack` can be very expensive

---

# Obsstore

- Not using revision numbers.

Problems

- No index - Pay $O(N)$ time loading all markers for anything accessing obsmarkers.

Complexities

- Need to lookup by predecessors or successors - multiple indexes needed

---

# Problem Summary

<!--column-->

File Storage:

|            | Revlog                  | Loose                 | Pack                  |
|------------|-------------------------|-----------------------|-----------------------|
| Revnum     | :cry:                   |:smiley:               |:smiley:               |
| Insertion  | :smiley:                |:slightly_smiling_face:|:thinking:             |
| Lookup     | :cry:                   |:smiley:               |:slightly_smiling_face:|
| Space      | :slightly_smiling_face: |:scream:               |:slightly_smiling_face:|
| Inode #    | :cry:                   |:scream:               |:smiley:               |
| Maintenance| :smiley:                |:smiley:               |:cry:                  |

<!--column-->
Obsstore:
- Multiple indexes

<br />

Changelog:
- Multiple indexes (nodemap, parent-child map)

---

# Indexed Log

Goals

- Decouple from revision numbers
- $O(\log N)$ insertion
- $O(\log N)$ lookup
- Avoid $O(N)$ in all cases except for fixing corruption
- No maintenance to keep above time complexity 
- Strong integrity 

---

# Indexed Log

Be general purposed.

![](1.jpg)

---

# Indexed Log


```bob,scale=0.8
.--------------------------------------------.
| File Storage                               |
|                                            |
| .-----------------------------.            |
| | Indexed Log                 |            |
| |                             |            |
| | .-------------------------. |            |
| | | Append Only Radix Index | |            |
| | |                         | |            |
| | | .-----------------.     | | .-------.  |
| | | | Integrity Check |     | | | Zstd  |  |
| | | | for append only |     | | | Delta |  |
| | | | files           |     | | '-------'  |
| | | '-----------------'     | |            |
| | '-------------------------' |            |
| '-----------------------------'            |
'--------------------------------------------'
```

---

# The Index

<!-- note: simplified -->

```bob
  Insert 81c2     |  Insert 82ee
                  |
    .----------------------.       .------.
    |             |        |       |      |
    v             |        |       |      v
  +-------------+ |  +---+-|-+---+-|-+ +-------------+
  | value: 81c2 | |  | 1 | * | 2 | * | | value: 82ee |
  +-------------+ |  +---+---+---+---+ +-------------+
    ^             |    ^
    |             |    |
    '---.         |    '---.
        |         |        |
  +---+-|-+       |  +---+-|-+
  | 8 | * |       |  | 8 | * |
  +---+---+       |  +---+---+
   Root v1        |   Root v2
```

---

# The Index

- Append-only Index + Atomic-replaced root pointer. Read is lock-free. 
- Keep modifications in-memory until an explicit `flush`.
- $O(\log N)$ insertion and lookup.
- No new files written. No maintenance required.

---

# The Log

- Stores a list of *entries*. An *entry* is a slice of `bytes`.
- Maintains checksum internally.

<!-- note: not SHA1 commit hash -->

---

# Indexed Log

- Indexed Log = Log (source of truth) + Indexes (cache)
- Define 0 or more *Index Functions* (`entry -> Vec<bytes>`)
- Indexed Log builds indexes automatically
- Indexes can be rebuilt purely from Log

<!-- note: no network access -->

---

# Indexed Log

On disk, an `IndexedLog` is stored as a directory:

- `log` The source of truth.
- `index.{foo}` Index "foo".
- `index.{foo}.sum` Chunked checksums of Index "foo". 
- `meta` Pointers to root nodes. Logical file lengths.


---

# Planned Use Cases

- File Storage
- Changelog Nodemap and Childmap
- Obsstore indexes
- Bookmark indexes
- Undo indexes

---

# Lightweight Transaction

With every data structure being append-only and controlled by `meta`. Transactions can be just different `meta` files ex. `meta.tr{name}`. This allows multiple on-going transactions.

---

# Q & A
