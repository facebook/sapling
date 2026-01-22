# Phase 4: Details Panel - Research

**Researched:** 2026-01-22
**Domain:** React UI component structure and layout
**Confidence:** HIGH

## Summary

The details panel is implemented in `CommitInfoView.tsx` and displays commit information in the right drawer. It contains two key sections: "Changes to amend" (lines 395-413) and "Files Changed" (lines 416-432). Currently both sections are rendered sequentially with no special prominence distinction beyond their order.

The current implementation uses:
- **Section components** from `utils.tsx` - simple wrapper with `commit-info-section` class
- **SmallCapsTitle** - uppercase labels for section headers
- **DiffStats component** - shows "X lines" (total significant LOC), not +X/-Y format
- **No collapsible functionality** - sections are always visible

**Primary recommendation:** Reorder sections (Files Changed above Changes to Amend), make Changes to Amend collapsible using existing `Collapsable` component, and enhance DiffStats to show +X/-Y format when data becomes available.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| React | 18.x | UI rendering | Project standard |
| Jotai | 2.x | State management | Already used for drawer state, selections |
| StyleX | (current) | Styling | Used in existing components |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| isl-components | (internal) | UI primitives | Badge, Icon, Divider components |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Collapsable component | Custom accordion | Collapsable already exists and is used in codebase |
| Section component | Custom div structure | Section provides consistent styling and semantics |

**Installation:**
No additional packages needed - all components exist in codebase.

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
├── CommitInfoView/
│   ├── CommitInfoView.tsx    # Main details panel
│   ├── DiffStats.tsx          # Line count display
│   ├── utils.tsx              # Section, SmallCapsTitle
│   └── CommitInfoView.css     # Styling
├── Collapsable.tsx            # Reusable collapsible section
└── UncommittedChanges.tsx     # Changes list component
```

### Pattern 1: Section Ordering for Visual Hierarchy
**What:** Place more important content higher in the layout
**When to use:** When de-emphasizing secondary content
**Example:**
```typescript
// Current order (lines 394-433 in CommitInfoView.tsx)
{commit.isDot && !isAmendDisabled ? (
  <Section data-testid="changes-to-amend">
    {/* Changes to amend section */}
  </Section>
) : null}
{isCommitMode ? null : (
  <Section data-testid="committed-changes">
    {/* Files changed section */}
  </Section>
)}

// Recommended order - Files Changed first
{isCommitMode ? null : (
  <Section data-testid="committed-changes">
    {/* Files changed section - now FIRST */}
  </Section>
)}
{commit.isDot && !isAmendDisabled ? (
  <Section data-testid="changes-to-amend">
    {/* Changes to amend section - now SECOND */}
  </Section>
) : null}
```

### Pattern 2: Collapsible Sections
**What:** Use existing `Collapsable` component for optional content
**When to use:** When content is secondary or optional viewing
**Example:**
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/Collapsable.tsx
import {Collapsable} from './Collapsable';

<Collapsable
  startExpanded={false}
  title={<SmallCapsTitle>Changes to Amend <Badge>...</Badge></SmallCapsTitle>}
  onToggle={(expanded) => {/* optional tracking */}}
>
  {/* Section content here */}
</Collapsable>
```

### Pattern 3: State Management with Jotai
**What:** Use Jotai atoms for persistent collapsed/expanded state
**When to use:** When state needs to persist across component remounts
**Example:**
```typescript
// Similar to generatedFilesInitiallyExpanded pattern (UncommittedChanges.tsx:353)
import {localStorageBackedAtom} from './jotaiUtils';

const amendSectionCollapsed = localStorageBackedAtom<boolean>(
  'isl.amend-section-collapsed',
  true  // default: collapsed
);
```

### Anti-Patterns to Avoid
- **Don't create custom collapsible logic** - `Collapsable` component already exists with proper accessibility
- **Don't use CSS display:none for de-emphasis** - Reordering and collapsing provide better UX
- **Don't modify Section component itself** - Wrap it in Collapsable instead

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Collapsible sections | Custom expand/collapse state + CSS transitions | `Collapsable` component | Already handles state, icons, animations, accessibility |
| Persistent UI preferences | localStorage + useState | `localStorageBackedAtom` from jotaiUtils | Type-safe, integrated with Jotai state |
| Section styling | Inline styles or custom CSS | `Section` component from utils.tsx | Consistent margins, semantic HTML |

**Key insight:** ISL has mature UI patterns already established. New features should compose existing components rather than creating parallel implementations.

## Common Pitfalls

### Pitfall 1: Breaking Existing Section Rendering
**What goes wrong:** Sections have specific data-testid attributes that tests depend on
**Why it happens:** Tests use `data-testid="changes-to-amend"` and `data-testid="committed-changes"` to find elements
**How to avoid:** Preserve data-testid attributes when restructuring, verify tests still pass
**Warning signs:** Test failures in `CommitInfoView.test.tsx`

### Pitfall 2: Conditional Rendering Conflicts
**What goes wrong:** Sections have complex conditional logic (`commit.isDot`, `isAmendDisabled`, `isCommitMode`)
**Why it happens:** Changes to amend only shows for HEAD commit in amend mode, Files Changed only in amend mode
**How to avoid:** Carefully preserve all conditional rendering logic when reordering
**Warning signs:** Sections appearing in wrong modes or for wrong commits

### Pitfall 3: Styling Inheritance Issues
**What goes wrong:** `commit-info-section` CSS class provides margins that may conflict with Collapsable
**Why it happens:** Collapsable adds its own container structure
**How to avoid:** Test visual spacing, may need to adjust margins when wrapping Section in Collapsable
**Warning signs:** Double margins, inconsistent spacing between sections

### Pitfall 4: Missing Line Count Data
**What goes wrong:** No +X/-Y data currently available in the type system
**Why it happens:** Current DiffStats only provides `sloc: number` (total significant lines), not separate added/deleted
**How to avoid:** Implement +X/-Y as enhancement when backend provides data, don't block on it
**Warning signs:** TypeScript errors when trying to access non-existent fields

## Code Examples

Verified patterns from official sources:

### Current Section Structure
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/CommitInfoView/CommitInfoView.tsx:395-413
<Section data-testid="changes-to-amend">
  <SmallCapsTitle>
    {isCommitMode ? <T>Changes to Commit</T> : <T>Changes to Amend</T>}
    <Badge>
      {selectedFilesLength === uncommittedChanges.length
        ? null
        : selectedFilesLength + '/'}
      {uncommittedChanges.length}
    </Badge>
  </SmallCapsTitle>
  {uncommittedChanges.length > 0 ? <PendingDiffStats /> : null}
  {uncommittedChanges.length === 0 ? (
    <Subtle>
      {isCommitMode ? <T>No changes to commit</T> : <T>No changes to amend</T>}
    </Subtle>
  ) : (
    <UncommittedChanges place={isCommitMode ? 'commit sidebar' : 'amend sidebar'} />
  )}
</Section>
```

### Current Files Changed Section
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/CommitInfoView/CommitInfoView.tsx:416-432
<Section data-testid="committed-changes">
  <SmallCapsTitle>
    <T>Files Changed</T>
    <Badge>{commit.totalFileCount}</Badge>
  </SmallCapsTitle>
  {commit.phase !== 'public' ? <DiffStats commit={commit} /> : null}
  <div className="changed-file-list">
    <div className="button-row">
      <OpenComparisonViewButton
        comparison={{type: ComparisonType.Committed, hash: commit.hash}}
      />
      <OpenAllFilesButton commit={commit} />
      <SplitButton trackerEventName="SplitOpenFromSplitSuggestion" commit={commit} />
    </div>
    <ChangedFilesWithFetching commit={commit} />
  </div>
</Section>
```

### Current DiffStats Implementation
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/CommitInfoView/DiffStats.tsx:40-49
export function DiffStats({commit}: Props) {
  const {slocInfo, isLoading} = useFetchSignificantLinesOfCode(commit);
  const significantLinesOfCode = slocInfo?.sloc;

  if (isLoading && significantLinesOfCode == null) {
    return <LoadingDiffStatsView />;
  } else if (!isLoading && significantLinesOfCode == null) {
    return null;
  }
  return <ResolvedDiffStatsView significantLinesOfCode={significantLinesOfCode} />;
}

// Current display (lines 72-86)
function ResolvedDiffStatsView({significantLinesOfCode}: {significantLinesOfCode: number | undefined}) {
  if (significantLinesOfCode == null) {
    return null;
  }
  return (
    <DiffStatsView>
      <T replace={{$num: significantLinesOfCode}}>$num lines</T>
    </DiffStatsView>
  );
}
```

### Collapsable Component Pattern
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/Collapsable.tsx
export function Collapsable({
  startExpanded,
  children,
  title,
  className,
  onToggle,
}: {
  startExpanded?: boolean;
  children: React.ReactNode;
  title: React.ReactNode;
  className?: string;
  onToggle?: (expanded: boolean) => unknown;
}) {
  const [isExpanded, setIsExpanded] = useState(startExpanded === true);
  return (
    <div className={'collapsable' + (className ? ` ${className}` : '')}>
      <div
        className="collapsable-title"
        onClick={() => {
          const newState = !isExpanded;
          setIsExpanded(newState);
          onToggle?.(newState);
        }}>
        <Icon icon={isExpanded ? 'chevron-down' : 'chevron-right'} /> {title}
      </div>
      {isExpanded ? <div className="collapsable-contents">{children}</div> : null}
    </div>
  );
}
```

### LocalStorage-Backed State Pattern
```typescript
// Source: /Users/jonas/code/sapling/addons/isl/src/UncommittedChanges.tsx:353-356
const generatedFilesInitiallyExpanded = localStorageBackedAtom<boolean>(
  'isl.expand-generated-files',
  false,
);

// Usage:
const [initiallyExpanded, setInitiallyExpanded] = useAtom(generatedFilesInitiallyExpanded);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| N/A - first implementation | Sequential section rendering | Existing | Need to reorder for prominence |
| N/A | Total lines (X lines) | Current | Need +X/-Y format for clarity |
| N/A | Always visible sections | Current | Need collapsible for de-emphasis |

**Deprecated/outdated:**
- None identified - this is new functionality being added to existing stable code

## Open Questions

Things that couldn't be fully resolved:

1. **+X/-Y Line Count Data Source**
   - What we know: Current `SlocInfo` type only has `sloc: number` (total significant lines)
   - What's unclear: Whether backend can provide separate added/deleted counts
   - Recommendation: Implement UI structure first with current data ("X lines"), enhance to +X/-Y when backend data available
   - Confidence: LOW - backend capabilities not researched

2. **Default Collapsed State for Changes to Amend**
   - What we know: Requirement says "collapsed by default or below files"
   - What's unclear: Which option preferred (collapsed vs moved below)
   - Recommendation: Implement both - move below AND make collapsible with collapsed default
   - Confidence: HIGH - provides maximum de-emphasis

3. **Visual Prominence for Files Changed**
   - What we know: Requirement says "larger, higher position"
   - What's unclear: Whether "larger" means font size, spacing, or visual weight
   - Recommendation: Higher position (reordering) likely sufficient, monitor user feedback
   - Confidence: MEDIUM - "larger" may be relative emphasis not literal size

## Sources

### Primary (HIGH confidence)
- `/Users/jonas/code/sapling/addons/isl/src/CommitInfoView/CommitInfoView.tsx` - Main details panel implementation
- `/Users/jonas/code/sapling/addons/isl/src/Collapsable.tsx` - Existing collapsible component
- `/Users/jonas/code/sapling/addons/isl/src/CommitInfoView/DiffStats.tsx` - Line count display
- `/Users/jonas/code/sapling/addons/isl/src/CommitInfoView/utils.tsx` - Section, SmallCapsTitle components
- `/Users/jonas/code/sapling/addons/isl/src/types.ts` - Type definitions (ChangedFile, SlocInfo)

### Secondary (MEDIUM confidence)
- [GitHub PR diff format](https://docs.github.com/articles/reviewing-proposed-changes-in-a-pull-request) - Verified +X/-Y is standard GitHub format
- [Graphite PR page redesign](https://graphite.com/blog/pr-page-redesign) - Confirms Graphite shows line counts in PR metadata

### Tertiary (LOW confidence)
- Backend capabilities for providing separate added/deleted line counts - not verified

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All components exist in codebase, verified by reading source
- Architecture: HIGH - Patterns extracted from actual codebase usage
- Pitfalls: HIGH - Identified from code structure and conditional rendering logic
- Line count format: MEDIUM - GitHub/Graphite standard verified, but backend data availability unclear

**Research date:** 2026-01-22
**Valid until:** 30 days (stable codebase, no fast-moving dependencies)
