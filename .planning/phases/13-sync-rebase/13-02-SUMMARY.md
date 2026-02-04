# Phase 13 Plan 02: Sync Warning Detection Summary

**One-liner:** localStorage-based detection system for pending comments and viewed files that will be affected by PR sync/rebase operations

---

## What Was Built

Created a warning check system that detects review state that will be invalidated by sync operations:

1. **syncWarning.ts module** - Core detection logic
   - `getSyncWarnings(prNumber, headHash)` - Checks both pending comments and viewed files
   - `SyncWarnings` type - Structured data with counts and hasWarnings flag
   - `formatSyncWarningMessage()` - User-friendly message formatting

2. **State detection logic:**
   - Pending comments: Read from `pendingCommentsAtom` (localStorage-backed Jotai atom)
   - Viewed files: Direct localStorage scan using prefix `isl.reviewed-files:pr:{prNumber}:{headHash}:`

3. **Module exports** - Added to reviewComments/index.ts for external access

**Key technical decisions:**
- Direct localStorage iteration for viewed file counting (matches how `reviewedFilesAtom` works)
- Correct prefix format: `isl.reviewed-files:` (with trailing 's')
- Documented SYN-05 behavior: pending comments persist through rebase but may become invalid
- Structured return data allows UI flexibility in presentation

---

## Tasks Completed

| Task | Description | Commit | Files |
|------|-------------|--------|-------|
| 1 | Create sync warning detection module | fc8d501 | syncWarning.ts |
| 2 | Export from reviewComments index | 4b81b5e | index.ts |

---

## Verification Results

✅ TypeScript compiles without errors
✅ Correct imports: `pendingCommentsAtom` from pendingCommentsState
✅ Correct localStorage prefix: `isl.reviewed-files:` matches atoms.ts
✅ Module exports properly from reviewComments/index.ts
✅ Function signatures match requirements (prNumber: string, headHash: string)

---

## Dependencies & Integration

**Requires:**
- Phase 10 (pendingCommentsAtom from 10-01)
- Phase 09 (reviewedFilesAtom key format from 09-01)

**Provides:**
- `getSyncWarnings()` - For UI components to check before sync
- `formatSyncWarningMessage()` - For consistent warning display
- `SyncWarnings` type - For type-safe warning data

**Affects:**
- 13-03: Sync operation UI will use these functions to show warnings
- 13-04: Rebase operation UI will use these functions to show warnings

---

## Key Technical Details

### localStorage Key Formats

**Pending comments:** `isl.pending-comments:{prNumber}`
- Managed by `localStorageBackedAtomFamily` with 7-day expiry
- Read via `readAtom(pendingCommentsAtom(prNumber))`

**Viewed files:** `isl.reviewed-files:pr:{prNumber}:{headHash}:{filePath}`
- Direct localStorage keys (not atom-managed)
- Scan all keys matching prefix to count viewed files
- headHash in key means new commits = all keys invalidated

### SYN-05: Pending Comments Persistence

Pending comments **ARE** persisted through rebase operations:
- Stored in localStorage with 7-day expiry
- Survive the rebase operation itself
- **However:** Line numbers may no longer match after rebase
- Warning informs users of this risk, but comments are NOT deleted
- Users can review and adjust pending comments after sync

---

## Code Quality

- ✅ TypeScript strict mode compliant
- ✅ Clear JSDoc documentation
- ✅ Consistent with existing code patterns
- ✅ No new lint warnings
- ✅ Proper error handling (null checks in localStorage iteration)

---

## Decisions Made

| Decision | Rationale | Impact |
|----------|-----------|--------|
| Direct localStorage iteration for viewed files | Matches pattern in reviewedFilesAtom, ensures accuracy | Same logic as storage mechanism |
| String prNumber parameter | Consistent with existing DiffId type (GitHub PR numbers are strings) | Type compatibility across modules |
| Separate pendingCommentCount and viewedFileCount | UI can show specific details or combined message | Flexibility for different warning styles |
| formatSyncWarningMessage utility | Consistent messaging across UI | Single source of truth for warning text |

---

## Next Phase Readiness

**Blockers:** None

**Recommendations for 13-03 (Sync Operation UI):**
1. Call `getSyncWarnings()` before triggering sync
2. Show warning modal if `hasWarnings` is true
3. Display formatted message using `formatSyncWarningMessage()`
4. Offer "Proceed anyway" and "Cancel" options
5. Consider showing pending comment details in expandable section

**Recommendations for 13-04 (Rebase Operation UI):**
- Same pattern as sync (warnings are identical for both operations)
- Can share common warning dialog component

---

## Metrics

- **Tasks completed:** 2/2
- **Files created:** 1 (syncWarning.ts)
- **Files modified:** 1 (index.ts)
- **Lines added:** 153
- **Commits:** 2
- **Duration:** ~2 minutes
- **Date:** 2026-02-02

---

## Testing Notes

**Manual testing needed (13-03/13-04):**
1. Create pending comments on a PR
2. Mark some files as viewed
3. Trigger sync operation
4. Verify warning shows correct counts
5. Verify pending comments persist after sync (but may be invalid)
6. Verify viewed files are reset after sync (new headHash)

**Edge cases handled:**
- No pending comments (count = 0)
- No viewed files (count = 0)
- Both warnings present (formatted with "and" conjunction)
- localStorage keys with non-"true" values (filtered out)

---

## Git Range

**Commits:**
- fc8d501: feat(13-02): add sync warning detection module
- 4b81b5e: feat(13-02): export sync warning functions from reviewComments

**Changed files:**
- `addons/isl/src/reviewComments/syncWarning.ts` (created)
- `addons/isl/src/reviewComments/index.ts` (modified)

---

## Deviations from Plan

None - plan executed exactly as written.

---

**Status:** ✅ Complete
**Phase:** 13-sync-rebase (Plan 02 of 05)
**Next:** 13-03 (Sync Operation UI)
