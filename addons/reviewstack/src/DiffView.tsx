/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange, Diff, ModifyChange} from './github/diffTypes';
import type {GitObjectID} from './github/types';

import SplitDiffView from './SplitDiffView';
import joinPath from './joinPath';
import {fileContentsDelta, gitHubBlob} from './recoil';
import {Box, Text} from '@primer/react';
import {useRecoilValueLoadable} from 'recoil';
import {FileHeader} from 'shared/SplitDiffView/SplitDiffFileHeader';

export default function DiffView({diff, isPullRequest}: {diff: Diff; isPullRequest: boolean}) {
  if (diff != null) {
    const children = diff.map(change => {
      const name = change.type === 'modify' ? change.before.name : change.entry.name;
      const key = `${change.basePath}/${name}`;
      return (
        <Box key={key} paddingY={1}>
          <ChangeDisplay change={change} isPullRequest={isPullRequest} />
        </Box>
      );
    });
    return <div>{children}</div>;
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
  const blobLoadable = useRecoilValueLoadable(gitHubBlob(oid));
  switch (blobLoadable.state) {
    case 'hasValue': {
      const {contents: blob} = blobLoadable;
      const {isBinary, text} = blob ?? {};
      if (text != null && !isBinary) {
        return (
          <SplitDiffView path={path} before={null} after={oid} isPullRequest={isPullRequest} />
        );
      } else {
        return <BinaryFile path={path} />;
      }
    }
    case 'loading': {
      return <div>{`loading patch for ${path}`}</div>;
    }
    case 'hasError': {
      return <div>{`error loading ${path}`}</div>;
    }
  }
}

function RemovedFile({basePath, name, oid}: {basePath: string; name: string; oid: GitObjectID}) {
  const path = joinPath(basePath, name);
  const blobLoadable = useRecoilValueLoadable(gitHubBlob(oid));
  switch (blobLoadable.state) {
    case 'hasValue': {
      return (
        <div>
          <FileHeader path={path} />
          <div className="patch-remove-line">File removed.</div>
        </div>
      );
    }
    case 'loading': {
      return <div>{`loading patch for ${path}`}</div>;
    }
    case 'hasError': {
      return <div>{`error loading ${path}`}</div>;
    }
  }
}

function ModifiedFile({modify, isPullRequest}: {modify: ModifyChange; isPullRequest: boolean}) {
  const {basePath, before, after} = modify;
  const path = joinPath(basePath, before.name);
  const fileMod = {
    before: before.oid,
    after: after.oid,
    path,
  };
  const fileModLoadable = useRecoilValueLoadable(fileContentsDelta(fileMod));
  switch (fileModLoadable.state) {
    case 'hasValue': {
      const {contents: delta} = fileModLoadable;
      const {before, after} = delta;
      if (before == null || after == null) {
        // Something went wrong?
        return null;
      }

      if (before.isBinary || after.isBinary) {
        // We could handle this more gracefully, particularly if only one of the
        // two files is binary, but this is good enough, for now.
        return <BinaryFile path={path} />;
      } else if (before.text == null || after.text == null) {
        // Something went wrong?
        return null;
      } else {
        return (
          <SplitDiffView
            path={path}
            before={before.oid}
            after={after.oid}
            isPullRequest={isPullRequest}
          />
        );
      }
    }
    case 'loading': {
      return <div>{`loading patch for ${path}`}</div>;
    }
    case 'hasError': {
      return <div>{`error loading ${path}`}</div>;
    }
  }
}

function BinaryFile({path}: {path: string}) {
  // TODO(mbolin): Check for special binary headers, such as PNG.
  return (
    <Box>
      <FileHeader path={path} />
      <Text>Binary file not shown.</Text>
    </Box>
  );
}
