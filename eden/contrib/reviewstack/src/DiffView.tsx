/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange, Diff, ModifyChange} from './github/diffTypes';
import type {GitObjectID} from './github/types';

import {FileHeader} from './SplitDiffFileHeader';
import SplitDiffView from './SplitDiffView';
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

export default function DiffView({diff, isPullRequest}: {diff: Diff; isPullRequest: boolean}) {
  if (diff != null) {
    return (
      <div>
        {diff.map(change => {
          const name = change.type === 'modify' ? change.before.name : change.entry.name;
          const key = `${change.basePath}/${name}`;
          return (
            <Suspense key={key} fallback={<DiffFileSkeleton />}>
              <Box paddingY={1}>
                <ChangeDisplay change={change} isPullRequest={isPullRequest} />
              </Box>
            </Suspense>
          );
        })}
      </div>
    );
  } else {
    return <div>commit not found or fetched from GitHub URL above</div>;
  }
}

function ChangeDisplay({change, isPullRequest}: {change: CommitChange; isPullRequest: boolean}) {
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
  }
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
    return (
      <SplitDiffView path={path} before={null} after={oid} isPullRequest={isPullRequest} />
    );
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
