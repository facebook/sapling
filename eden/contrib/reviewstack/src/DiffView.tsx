/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Diff, ModifyChange} from './github/diffTypes';
import type {CommitComparisonFile} from './github/restApiTypes';
import type {GitObjectID} from './github/types';

import {FileHeader} from './SplitDiffFileHeader';
import SplitDiffView from './SplitDiffView';
import coalesceRenamedFiles, {type DisplayChange, type RenamedFile} from './coalesceRenamedFiles';
import {diffFileAnchorID, getDisplayChangePath} from './diffFileNavigation';
import hasBinaryContent from './hasBinaryContent';
import joinPath from './joinPath';
import {fileContentsDeltaAtom, gitHubBlobAtom} from './jotai/atoms';
import {Box, Text} from '@primer/react';
import {useAtomValue} from 'jotai';
import React, {Suspense, useMemo} from 'react';

function DiffFileSkeleton(): React.ReactElement {
  return (
    <Box
      borderWidth="1px"
      borderStyle="solid"
      borderColor="border.default"
      borderRadius={2}
      padding={3}
      bg="canvas.subtle">
      <Box height={20} width="60%" bg="neutral.muted" borderRadius={1} />
    </Box>
  );
}

export default function DiffView({
  diff,
  isPullRequest,
  comparisonFiles = [],
}: {
  diff: Diff;
  isPullRequest: boolean;
  comparisonFiles?: readonly CommitComparisonFile[];
}) {
  if (diff != null) {
    return (
      <div>
        {coalesceRenamedFiles(diff, comparisonFiles).map(change => {
          const path = getDisplayChangePath(change);
          const key =
            change.type === 'rename'
              ? `rename:${change.before.basePath}/${change.before.entry.name}:${change.after.basePath}/${change.after.entry.name}`
              : `${change.basePath}/${
                  change.type === 'modify' ? change.before.name : change.entry.name
                }`;
          return (
            <Box key={key} id={diffFileAnchorID(path)} paddingY={1} sx={{scrollMarginTop: '8px'}}>
              <Suspense fallback={<DiffFileSkeleton />}>
                <ChangeDisplay change={change} isPullRequest={isPullRequest} />
              </Suspense>
            </Box>
          );
        })}
      </div>
    );
  } else {
    return <div>commit not found or fetched from GitHub URL above</div>;
  }
}

function ChangeDisplay({change, isPullRequest}: {change: DisplayChange; isPullRequest: boolean}) {
  switch (change.type) {
    case 'add': {
      const {basePath, entry} = change;
      const {name, oid} = entry;
      return <AddedFile basePath={basePath} name={name} oid={oid} isPullRequest={isPullRequest} />;
    }
    case 'remove': {
      const {basePath, entry} = change;
      const {name, oid} = entry;
      return <RemovedFile basePath={basePath} name={name} oid={oid} />;
    }
    case 'modify': {
      return <ModifiedFile modify={change} isPullRequest={isPullRequest} />;
    }
    case 'rename': {
      return <RenamedFileDisplay rename={change} isPullRequest={isPullRequest} />;
    }
  }
}

function RenamedFileDisplay({
  rename,
  isPullRequest,
}: {
  rename: RenamedFile;
  isPullRequest: boolean;
}) {
  const previousPath = joinPath(rename.before.basePath, rename.before.entry.name);
  const path = joinPath(rename.after.basePath, rename.after.entry.name);
  const before = rename.before.entry;
  const after = rename.after.entry;

  if (before.oid === after.oid && before.mode === after.mode) {
    return (
      <Box borderWidth="1px" borderStyle="solid" borderColor="border.default" borderRadius={2}>
        <FileHeader path={path} previousPath={previousPath} />
      </Box>
    );
  }

  return (
    <ModifiedRenamedFileDisplay
      after={after}
      before={before}
      isPullRequest={isPullRequest}
      path={path}
      previousPath={previousPath}
    />
  );
}

function ModifiedRenamedFileDisplay({
  after,
  before,
  isPullRequest,
  path,
  previousPath,
}: {
  after: RenamedFile['after']['entry'];
  before: RenamedFile['before']['entry'];
  isPullRequest: boolean;
  path: string;
  previousPath: string;
}) {
  const fileMod = useMemo(
    () => ({before: before.oid, after: after.oid, path}),
    [after.oid, before.oid, path],
  );
  const fileModAtom = useMemo(() => fileContentsDeltaAtom(fileMod), [fileMod]);
  const delta = useAtomValue(fileModAtom);
  const {before: beforeBlob, after: afterBlob} = delta;

  if (beforeBlob == null || afterBlob == null) {
    return null;
  }
  if (
    beforeBlob.isBinary ||
    afterBlob.isBinary ||
    hasBinaryContent(beforeBlob.text) ||
    hasBinaryContent(afterBlob.text) ||
    beforeBlob.text == null ||
    afterBlob.text == null
  ) {
    return (
      <Box borderWidth="1px" borderStyle="solid" borderColor="border.default" borderRadius={2}>
        <FileHeader path={path} previousPath={previousPath} />
        <Text padding={3}>Binary file not shown.</Text>
      </Box>
    );
  }
  return (
    <SplitDiffView
      path={path}
      previousPath={previousPath}
      before={beforeBlob.oid}
      after={afterBlob.oid}
      isPullRequest={isPullRequest}
    />
  );
}

function AddedFile({
  basePath,
  name,
  oid,
  isPullRequest,
}: {
  basePath: string;
  name: string;
  oid: GitObjectID;
  isPullRequest: boolean;
}) {
  const path = joinPath(basePath, name);
  const blobAtom = useMemo(() => gitHubBlobAtom(oid), [oid]);
  const blob = useAtomValue(blobAtom);
  const {isBinary, text} = blob ?? {};
  // Check both the isBinary flag and perform our own binary content detection
  if (text != null && !isBinary && !hasBinaryContent(text)) {
    return <SplitDiffView path={path} before={null} after={oid} isPullRequest={isPullRequest} />;
  } else {
    return <BinaryFile path={path} />;
  }
}

function RemovedFile({basePath, name, oid}: {basePath: string; name: string; oid: GitObjectID}) {
  const path = joinPath(basePath, name);
  const blobAtom = useMemo(() => gitHubBlobAtom(oid), [oid]);
  // useAtomValue will suspend until the blob is loaded
  useAtomValue(blobAtom);
  return (
    <div>
      <FileHeader path={path} />
      <div className="patch-remove-line">File removed.</div>
    </div>
  );
}

function ModifiedFile({modify, isPullRequest}: {modify: ModifyChange; isPullRequest: boolean}) {
  const {basePath, before, after} = modify;
  const path = joinPath(basePath, before.name);
  const fileMod = useMemo(
    () => ({
      before: before.oid,
      after: after.oid,
      path,
    }),
    [before.oid, after.oid, path],
  );
  const fileModAtom = useMemo(() => fileContentsDeltaAtom(fileMod), [fileMod]);
  const delta = useAtomValue(fileModAtom);
  const {before: beforeBlob, after: afterBlob} = delta;
  if (beforeBlob == null || afterBlob == null) {
    // Something went wrong?
    return null;
  }

  // Check both the isBinary flag and perform our own binary content detection
  if (
    beforeBlob.isBinary ||
    afterBlob.isBinary ||
    hasBinaryContent(beforeBlob.text) ||
    hasBinaryContent(afterBlob.text)
  ) {
    // We could handle this more gracefully, particularly if only one of the
    // two files is binary, but this is good enough, for now.
    return <BinaryFile path={path} />;
  } else if (beforeBlob.text == null || afterBlob.text == null) {
    // Something went wrong?
    return null;
  } else {
    return (
      <SplitDiffView
        path={path}
        before={beforeBlob.oid}
        after={afterBlob.oid}
        isPullRequest={isPullRequest}
      />
    );
  }
}

function BinaryFile({path}: {path: string}) {
  return (
    <Box>
      <FileHeader path={path} />
      <Text>Binary file not shown.</Text>
    </Box>
  );
}
