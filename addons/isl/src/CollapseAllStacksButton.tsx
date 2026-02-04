/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom, useAtomValue} from 'jotai';
import {collapsedStacksAtom} from './StackActions';
import {dagWithPreviews} from './previews';
import {t} from './i18n';

/**
 * Button to collapse or expand all draft stacks in the commit tree.
 */
export function CollapseAllStacksButton() {
  const dag = useAtomValue(dagWithPreviews);
  const [collapsedStacks, setCollapsedStacks] = useAtom(collapsedStacksAtom);

  // Find all draft stack roots (draft commits whose parents are not drafts)
  const draftStackRoots: string[] = [];
  for (const commit of dag.values()) {
    if (commit.phase === 'draft') {
      const parentHashes = commit.parents;
      const hasNonDraftParent =
        parentHashes.length === 0 ||
        parentHashes.some(parentHash => {
          const parent = dag.get(parentHash);
          return parent == null || parent.phase !== 'draft';
        });
      // Only include if the stack has children (otherwise nothing to collapse)
      if (hasNonDraftParent && dag.children(commit.hash).size > 0) {
        draftStackRoots.push(commit.hash);
      }
    }
  }

  // If no stacks with children, don't show the button
  if (draftStackRoots.length === 0) {
    return null;
  }

  const allCollapsed = draftStackRoots.every(hash => collapsedStacks.includes(hash));

  const handleClick = () => {
    if (allCollapsed) {
      // Expand all: remove all draft stack roots from collapsed list
      setCollapsedStacks(prev => prev.filter(hash => !draftStackRoots.includes(hash)));
    } else {
      // Collapse all: add all draft stack roots to collapsed list
      setCollapsedStacks(prev => {
        const newSet = new Set(prev);
        for (const hash of draftStackRoots) {
          newSet.add(hash);
        }
        return [...newSet];
      });
    }
  };

  return (
    <Tooltip title={allCollapsed ? t('Expand all stacks') : t('Collapse all stacks')}>
      <Button icon onClick={handleClick} data-testid="collapse-all-stacks-button">
        <Icon icon={allCollapsed ? 'unfold' : 'fold'} />
      </Button>
    </Tooltip>
  );
}
