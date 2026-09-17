/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';
import type {PullRequestFileTreeNode} from './pullRequestFileTree';

import coalesceRenamedFiles from './coalesceRenamedFiles';
import {getDisplayChangeLabel, scrollToDiffFile} from './diffFileNavigation';
import {gitHubPullRequestComparisonFilesAtom, gitHubPullRequestVersionDiffAtom} from './jotai';
import buildPullRequestFileTree from './pullRequestFileTree';
import {ChevronDownIcon, ChevronRightIcon, FileDirectoryIcon} from '@primer/octicons-react';
import {Box, Button, StyledOcticon, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import {useMemo, useState} from 'react';

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
  const tree = useMemo(() => buildPullRequestFileTree(changes), [changes]);
  const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(() => new Set());

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
      return (
        <Button
          key={`${node.change.type}:${node.path}`}
          aria-label={`Go to ${label}`}
          onClick={() => scrollToDiffFile(node.path)}
          title={label}
          variant="invisible"
          sx={{
            alignItems: 'center',
            borderRadius: 0,
            display: 'flex',
            fontWeight: 'normal',
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
      <Box overflow="auto" paddingY={1}>
        {renderNodes(tree)}
      </Box>
    </Box>
  );
}
