/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Repository} from '../Repository';
import type {PathLike} from 'fs';
import type {FileHandle} from 'fs/promises';

import {GeneratedFilesDetector} from '../GeneratedFiles';
import {promises} from 'fs';
import {GeneratedStatus} from 'isl/src/types';
import {mockLogger} from 'shared/testUtils';

/* eslint-disable require-await */

describe('GeneratedFiles', () => {
  describe('getGeneratedFilePathRegex', () => {
    it('can take configured custom regex', async () => {
      jest.spyOn(promises, 'open').mockImplementation(async () => {
        throw new Error('skipping in tests');
      });

      const mockRepo = {
        getConfig: async () => Promise.resolve('foobar'),
        logger: mockLogger,
      } as unknown as Repository;
      const detector = new GeneratedFilesDetector();
      const result = await detector.queryFilesGenerated(mockRepo, mockLogger, '/', [
        'src/myFile.js',
        'foobar',
      ]);
      expect(result).toEqual({
        'src/myFile.js': GeneratedStatus.Manual,
        foobar: GeneratedStatus.Generated,
      });
    });

    it('detects yarn.lock as generated', async () => {
      jest.spyOn(promises, 'open').mockImplementation(async () => {
        throw new Error('skipping in tests');
      });

      const mockRepo = {
        getConfig: async () => Promise.resolve(undefined),
        logger: mockLogger,
      } as unknown as Repository;
      const detector = new GeneratedFilesDetector();
      const result = await detector.queryFilesGenerated(mockRepo, mockLogger, '/', [
        'src/myFile.js',
        'yarn.lock',
        'subproject/yarn.lock',
      ]);
      expect(result).toEqual({
        'src/myFile.js': GeneratedStatus.Manual,
        'yarn.lock': GeneratedStatus.Generated,
        'subproject/yarn.lock': GeneratedStatus.Generated,
      });
    });
  });

  describe('readFilesLookingForGeneratedTag', () => {
    it('detects generate tag in file content', async () => {
      jest.spyOn(promises, 'open').mockImplementation(async (filePath: PathLike, _flags, _mod) => {
        return {
          read: jest.fn(async () => ({
            buffer:
              filePath === '/myGeneratedFile.js'
                ? `/* this file is ${'@'}generated */`
                : filePath === '/myPartiallyGeneratedFile.js'
                ? `/* this file is ${'@'}partially-generated */`
                : '// Normal file content',
          })),
          close: jest.fn(),
        } as unknown as FileHandle;
      });

      const mockRepo = {
        getConfig: async () => Promise.resolve(undefined),
        logger: mockLogger,
      } as unknown as Repository;
      const detector = new GeneratedFilesDetector();
      const result = await detector.queryFilesGenerated(mockRepo, mockLogger, '/', [
        'myFile.js',
        'myPartiallyGeneratedFile.js',
        'myGeneratedFile.js',
      ]);
      expect(result).toEqual({
        'myFile.js': GeneratedStatus.Manual,
        'myPartiallyGeneratedFile.js': GeneratedStatus.PartiallyGenerated,
        'myGeneratedFile.js': GeneratedStatus.Generated,
      });
    });
  });
});
