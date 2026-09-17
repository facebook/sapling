/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';
import type {PullRequestFileTreeNode} from './pullRequestFileTree';

import coalesceRenamedFiles from './coalesceRenamedFiles';
import {
  diffFileAnchorID,
  getDisplayChangeLabel,
  getDisplayChangePath,
  scrollToDiffFile,
} from './diffFileNavigation';
import {gitHubPullRequestComparisonFilesAtom, gitHubPullRequestVersionDiffAtom} from './jotai';
import buildPullRequestFileTree from './pullRequestFileTree';
import {ChevronDownIcon, ChevronRightIcon, FileDirectoryIcon} from '@primer/octicons-react';
import {Box, Button, StyledOcticon, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import {useEffect, useMemo, useRef, useState} from 'react';

const symbolForChange: Record<DisplayChange['type'], string> = {
  add: '+',
  modify: '●',
  remove: '−',
  rename: '↪',
};

const colorForChange: Record<DisplayChange['type'], string> = {
  add: 'success.fg',
  modify: 'attention.fg',
  remove: 'danger.fg',
  rename: 'fg.muted',
};

const labelForChange: Record<DisplayChange['type'], string> = {
  add: 'New',
  modify: 'Modified',
  remove: 'Removed',
  rename: 'Moved or renamed',
};

function FileStats({
  additions,
  deletions,
}: {
  additions?: number;
  deletions?: number;
}): React.ReactElement | null {
  if (additions == null || deletions == null) {
    return null;
  }
  return (
    <Box display="flex" flexShrink={0} marginLeft="auto" sx={{gap: '5px'}}>
      <Text color="success.fg" fontFamily="mono" fontSize={0}>
        +{additions}
      </Text>
      <Text color="danger.fg" fontFamily="mono" fontSize={0}>
        −{deletions}
      </Text>
    </Box>
  );
}

export default function PullRequestFiles(): React.ReactElement {
  const diff = useAtomValue(gitHubPullRequestVersionDiffAtom);
  const comparisonFiles = useAtomValue(gitHubPullRequestComparisonFilesAtom);
  const fileInfoByPath = useMemo(
    () => new Map(comparisonFiles.map(file => [file.filename, file])),
    [comparisonFiles],
  );
  const changes = useMemo(
    () => (diff == null ? [] : coalesceRenamedFiles(diff.diff, comparisonFiles)),
    [comparisonFiles, diff],
  );
  const filePaths = useMemo(() => changes.map(getDisplayChangePath), [changes]);
  const tree = useMemo(() => buildPullRequestFileTree(changes), [changes]);
  const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(() => new Set());
  const [activePath, setActivePath] = useState<string | null>(() => filePaths[0] ?? null);
  const fileListRef = useRef<HTMLDivElement>(null);
  const fileRowRefs = useRef(new Map<string, HTMLButtonElement>());

  useEffect(() => {
    const scrollContainer = document.querySelector<HTMLElement>(
      '[data-reviewstack-diff-scroll="true"]',
    );
    if (scrollContainer == null) {
      return;
    }

    let animationFrame: number | null = null;
    const updateActivePath = () => {
      if (animationFrame != null) {
        cancelAnimationFrame(animationFrame);
      }
      animationFrame = requestAnimationFrame(() => {
        animationFrame = null;
        const viewportTop = scrollContainer.getBoundingClientRect().top + 8;
        let nextPath = filePaths[0] ?? null;
        for (const path of filePaths) {
          const file = document.getElementById(diffFileAnchorID(path));
          if (file == null) {
            continue;
          }
          if (file.getBoundingClientRect().top > viewportTop) {
            break;
          }
          nextPath = path;
        }
        setActivePath(current => (current === nextPath ? current : nextPath));
      });
    };

    const mutationObserver = new MutationObserver(updateActivePath);
    mutationObserver.observe(scrollContainer, {childList: true, subtree: true});
    scrollContainer.addEventListener('scroll', updateActivePath, {passive: true});
    window.addEventListener('resize', updateActivePath);
    updateActivePath();

    return () => {
      if (animationFrame != null) {
        cancelAnimationFrame(animationFrame);
      }
      mutationObserver.disconnect();
      scrollContainer.removeEventListener('scroll', updateActivePath);
      window.removeEventListener('resize', updateActivePath);
    };
  }, [filePaths]);

  useEffect(() => {
    if (activePath == null) {
      return;
    }
    const list = fileListRef.current;
    const row = fileRowRefs.current.get(activePath);
    if (list == null || row == null) {
      return;
    }
    const listRect = list.getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    if (rowRect.top < listRect.top) {
      list.scrollTop -= listRect.top - rowRect.top;
    } else if (rowRect.bottom > listRect.bottom) {
      list.scrollTop += rowRect.bottom - listRect.bottom;
    }
  }, [activePath, collapsedDirectories]);

  const toggleDirectory = (path: string) => {
    setCollapsedDirectories(current => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const renderNodes = (nodes: PullRequestFileTreeNode[], depth = 0): React.ReactNode =>
    nodes.map(node => {
      if (node.kind === 'directory') {
        const collapsed = collapsedDirectories.has(node.path);
        return (
          <Box key={`directory:${node.path}`}>
            <Button
              aria-expanded={!collapsed}
              aria-label={`${collapsed ? 'Expand' : 'Collapse'} ${node.path}`}
              onClick={() => toggleDirectory(node.path)}
              title={node.path}
              variant="invisible"
              sx={{
                alignItems: 'center',
                borderRadius: 0,
                display: 'flex',
                fontWeight: 'normal',
                height: 28,
                justifyContent: 'flex-start',
                paddingLeft: `${8 + depth * 12}px`,
                paddingRight: 2,
                width: '100%',
                '& > [data-component="text"]': {
                  alignItems: 'center',
                  display: 'flex',
                  minWidth: 0,
                  width: '100%',
                },
              }}>
              <StyledOcticon icon={collapsed ? ChevronRightIcon : ChevronDownIcon} size={12} />
              <StyledOcticon icon={FileDirectoryIcon} color="fg.muted" sx={{marginLeft: 1}} />
              <Text
                fontSize={1}
                marginLeft={1}
                sx={{overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap'}}>
                {node.name}
              </Text>
            </Button>
            {collapsed ? null : renderNodes(node.children, depth + 1)}
          </Box>
        );
      }

      const label = getDisplayChangeLabel(node.change);
      const isActive = node.path === activePath;
      return (
        <Button
          key={`${node.change.type}:${node.path}`}
          aria-label={`Go to ${label}`}
          aria-current={isActive ? 'location' : undefined}
          data-active={isActive ? 'true' : undefined}
          data-file-path={node.path}
          onClick={() => scrollToDiffFile(node.path)}
          ref={element => {
            if (element == null) {
              fileRowRefs.current.delete(node.path);
            } else {
              fileRowRefs.current.set(node.path, element);
            }
          }}
          title={label}
          variant="invisible"
          sx={{
            alignItems: 'center',
            backgroundColor: isActive ? 'accent.subtle' : 'transparent',
            borderLeftColor: isActive ? 'accent.emphasis' : 'transparent',
            borderLeftStyle: 'solid',
            borderLeftWidth: '3px',
            borderRadius: 0,
            display: 'flex',
            fontWeight: isActive ? 'semibold' : 'normal',
            height: 28,
            justifyContent: 'flex-start',
            paddingLeft: `${28 + depth * 12}px`,
            paddingRight: 2,
            width: '100%',
            '& > [data-component="text"]': {
              alignItems: 'center',
              display: 'flex',
              minWidth: 0,
              width: '100%',
            },
          }}>
          <Text
            aria-label={`${labelForChange[node.change.type]} file`}
            color={colorForChange[node.change.type]}
            fontFamily="mono"
            fontSize={0}
            fontWeight="bold"
            sx={{flexShrink: 0, textAlign: 'center', width: '14px'}}>
            {symbolForChange[node.change.type]}
          </Text>
          <Text
            fontFamily="mono"
            fontSize={0}
            marginLeft={2}
            sx={{
              flex: 1,
              minWidth: 0,
              overflow: 'hidden',
              textAlign: 'left',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}>
            {node.name}
          </Text>
          <FileStats
            additions={fileInfoByPath.get(node.path)?.additions}
            deletions={fileInfoByPath.get(node.path)?.deletions}
          />
        </Button>
      );
    });

  return (
    <Box display="flex" flexDirection="column" height="100%" overflow="hidden">
      <Box
        borderBottomColor="border.default"
        borderBottomStyle="solid"
        borderBottomWidth={1}
        flexShrink={0}
        padding={2}>
        <Text fontSize={1} fontWeight="bold">
          Files changed
        </Text>
        <Text as="div" color="fg.muted" fontSize={0}>
          {changes.length} {changes.length === 1 ? 'file' : 'files'}
        </Text>
        <Box display="flex" flexWrap="wrap" marginTop={1} sx={{columnGap: '10px', rowGap: '2px'}}>
          <Text color="success.fg" fontSize={0}>
            + new
          </Text>
          <Text color="attention.fg" fontSize={0}>
            ● modified
          </Text>
          <Text color="danger.fg" fontSize={0}>
            − removed
          </Text>
          <Text color="fg.muted" fontSize={0}>
            ↪ moved/renamed
          </Text>
        </Box>
      </Box>
      <Box
        ref={fileListRef}
        data-testid="pull-request-file-list"
        flexGrow={1}
        minHeight={0}
        overflowY="scroll"
        paddingY={1}
        sx={{scrollbarGutter: 'stable'}}>
        {renderNodes(tree)}
      </Box>
    </Box>
  );
}
