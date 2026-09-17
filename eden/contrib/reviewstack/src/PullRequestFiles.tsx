/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DisplayChange} from './coalesceRenamedFiles';

import coalesceRenamedFiles from './coalesceRenamedFiles';
import {getDisplayChangeLabel, getDisplayChangePath, scrollToDiffFile} from './diffFileNavigation';
import {gitHubPullRequestVersionDiffAtom} from './jotai';
import {
  DiffAddedIcon,
  DiffModifiedIcon,
  DiffRemovedIcon,
  DiffRenamedIcon,
} from '@primer/octicons-react';
import {Box, Button, StyledOcticon, Text} from '@primer/react';
import {useAtomValue} from 'jotai';

const iconForChange: Record<DisplayChange['type'], React.ElementType> = {
  add: DiffAddedIcon,
  modify: DiffModifiedIcon,
  remove: DiffRemovedIcon,
  rename: DiffRenamedIcon,
};

const colorForChange: Record<DisplayChange['type'], string> = {
  add: 'success.fg',
  modify: 'attention.fg',
  remove: 'danger.fg',
  rename: 'fg.muted',
};

export default function PullRequestFiles(): React.ReactElement {
  const diff = useAtomValue(gitHubPullRequestVersionDiffAtom);
  const changes = diff == null ? [] : coalesceRenamedFiles(diff.diff);

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
      </Box>
      <Box overflow="auto" paddingY={1}>
        {changes.map(change => {
          const path = getDisplayChangePath(change);
          const label = getDisplayChangeLabel(change);
          const Icon = iconForChange[change.type];
          return (
            <Button
              key={`${change.type}:${label}`}
              aria-label={`Go to ${label}`}
              onClick={() => scrollToDiffFile(path)}
              title={label}
              variant="invisible"
              sx={{
                alignItems: 'flex-start',
                borderRadius: 0,
                display: 'flex',
                justifyContent: 'flex-start',
                paddingX: 2,
                paddingY: 1,
                width: '100%',
              }}>
              <StyledOcticon icon={Icon} color={colorForChange[change.type]} sx={{marginTop: 1}} />
              <Text
                fontFamily="mono"
                fontSize={0}
                marginLeft={2}
                sx={{overflowWrap: 'anywhere', textAlign: 'left', whiteSpace: 'normal'}}>
                {label}
              </Text>
            </Button>
          );
        })}
      </Box>
    </Box>
  );
}
