/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CodeReviewSystem} from '../types';

import App from '../App';
import {CommitInfoTestUtils} from '../testQueries';
import {
  COMMIT,
  expectMessageNOTSentToServer,
  expectMessageSentToServer,
  fireMouseEvent,
  getLastBinaryMessageSentToServer,
  resetTestMessages,
  simulateCommits,
  simulateMessageFromServer,
  simulateUncommittedChangedFiles,
} from '../testUtils';
import {fireEvent, render, waitFor, screen, act} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {nextTick} from 'shared/testUtils';
import * as utils from 'shared/utils';

describe('Image upload inside TextArea ', () => {
  beforeEach(() => {
    resetTestMessages();
  });

  beforeEach(() => {
    render(<App />);
    act(() => {
      expectMessageSentToServer({
        type: 'subscribe',
        kind: 'smartlogCommits',
        subscriptionID: expect.anything(),
      });
      simulateCommits({
        value: [
          COMMIT('1', 'some public base', '0', {phase: 'public'}),
          COMMIT('b', 'My Commit', '1'),
          COMMIT('a', 'My Commit', 'b', {isDot: true}),
        ],
      });
    });
    act(() => {
      CommitInfoTestUtils.clickToEditTitle();
      CommitInfoTestUtils.clickToEditDescription();
    });
  });

  const mockFile = {
    name: 'file.png',
    arrayBuffer: () => Promise.resolve(new Uint8Array([0, 1, 2]).buffer),
  } as File;

  const dataTransfer = {
    files: [mockFile] as unknown as FileList,
  } as DataTransfer;

  describe('Drag and drop image', () => {
    it('renders highlight while dragging image', () => {
      const textarea = CommitInfoTestUtils.getDescriptionEditor();

      act(() => void fireMouseEvent('dragenter', textarea, 0, 0, {dataTransfer}));
      expect(document.querySelector('.hovering-to-drop')).not.toBeNull();
      act(() => void fireMouseEvent('dragleave', textarea, 0, 0, {dataTransfer}));
      expect(document.querySelector('.hovering-to-drop')).toBeNull();
    });

    it('does not try to upload other things being dragged', () => {
      const textarea = CommitInfoTestUtils.getDescriptionEditor();
      act(() => {
        fireMouseEvent('dragenter', textarea, 0, 0, {
          dataTransfer: {
            files: [],
            items: [],
          } as unknown as DataTransfer,
        });
      }); // drag without files is ignored
      expect(document.querySelector('.hovering-to-drop')).toBeNull();
    });

    it('lets you drag an image to upload it', async () => {
      const textarea = CommitInfoTestUtils.getDescriptionEditor();
      act(() => void fireMouseEvent('dragenter', textarea, 0, 0, {dataTransfer}));
      act(() => {
        fireMouseEvent('drop', textarea, 0, 0, {dataTransfer});
      });

      await waitFor(() => {
        expectMessageSentToServer(expect.objectContaining({type: 'uploadFile'}));
      });
    });
  });

  describe('Paste image to upload', () => {
    it('lets you paste an image to upload it', async () => {
      const textarea = CommitInfoTestUtils.getDescriptionEditor();
      act(() => {
        fireEvent.paste(textarea, {clipboardData: dataTransfer});
      });
      await waitFor(() => {
        expectMessageSentToServer(expect.objectContaining({type: 'uploadFile'}));
      });
    });
    it('pastes without images are handled normally', async () => {
      const textarea = CommitInfoTestUtils.getDescriptionEditor();
      act(() => void fireEvent.paste(textarea));
      await nextTick(); // allow file upload to await arrayBuffer()
      expectMessageNOTSentToServer(expect.objectContaining({type: 'uploadFile'}));
    });
  });

  describe('file picker to upload file', () => {
    it('lets you pick a file to upload', async () => {
      const uploadButton = screen.getAllByTestId('attach-file-input')[0];
      act(() => {
        userEvent.upload(uploadButton, [mockFile]);
      });

      await waitFor(() => {
        expectMessageSentToServer(expect.objectContaining({type: 'uploadFile'}));
      });
    });
  });

  describe('Image upload UI', () => {
    async function startFileUpload(id: string) {
      const randomIdSpy = jest.spyOn(utils, 'randomId');
      randomIdSpy.mockImplementationOnce(() => id);
      const uploadButton = screen.getAllByTestId('attach-file-input')[0];
      act(() => void userEvent.upload(uploadButton, [mockFile]));
      await waitFor(() =>
        expectMessageSentToServer(expect.objectContaining({type: 'uploadFile', id})),
      );
      randomIdSpy.mockRestore();
    }

    async function simulateUploadSucceeded(id: string) {
      await act(async () => {
        simulateMessageFromServer({
          type: 'uploadFileResult',
          id,
          result: {value: `https://image.example.com/${id}`},
        });
        await nextTick();
      });
    }

    async function simulateUploadFailed(id: string) {
      await act(async () => {
        simulateMessageFromServer({
          type: 'uploadFileResult',
          id,
          result: {error: new Error('upload failed')},
        });
        await nextTick();
      });
    }

    const {descriptionTextContent, getDescriptionEditor} = CommitInfoTestUtils;

    it('shows placeholder when uploading an image', async () => {
      expect(descriptionTextContent()).not.toContain('Uploading');
      await startFileUpload('1111');
      expect(descriptionTextContent()).toContain('Uploading #1');
    });

    it('sends a message to the server to upload the file', async () => {
      jest.spyOn(utils, 'randomId').mockImplementation(() => '1111');
      await startFileUpload('1111');
      expectMessageSentToServer({
        type: 'uploadFile',
        filename: 'file.png',
        hasBinaryPayload: true,
        id: '1111',
      });
      const binary = getLastBinaryMessageSentToServer();
      expect(binary).toEqual(new Uint8Array([1, 2, 3, 4]).buffer);
    });

    it('removes placeholder when upload succeeds', async () => {
      await startFileUpload('1111');
      expect(descriptionTextContent()).toContain('Uploading #1');
      await simulateUploadSucceeded('1111');
      expect(descriptionTextContent()).not.toContain('Uploading #1');
      expect(descriptionTextContent()).toContain('https://image.example.com/1111');
    });

    it('removes placeholder when upload fails', async () => {
      await startFileUpload('1111');
      expect(descriptionTextContent()).toContain('Uploading #1');
      await simulateUploadFailed('1111');
      expect(descriptionTextContent()).not.toContain('Uploading #1');
      expect(descriptionTextContent()).not.toContain('https://image.example.com');
    });

    it('shows progress of ongoing uploads', async () => {
      await startFileUpload('1111');
      expect(screen.getByText('Uploading 1 file')).toBeInTheDocument();
    });

    it('click to cancel upload', async () => {
      await startFileUpload('1111');
      expect(screen.getByText('Uploading 1 file')).toBeInTheDocument();
      act(() => {
        fireEvent.mouseOver(screen.getByText('Uploading 1 file'));
      });
      expect(screen.getByText('Click to cancel')).toBeInTheDocument();
      act(() => {
        fireEvent.click(screen.getByText('Click to cancel'));
      });

      expect(descriptionTextContent()).not.toContain('Uploading #1');
      expect(screen.queryByText('Uploading 1 file')).not.toBeInTheDocument();
    });

    it('clears hover state when cancelling', async () => {
      await startFileUpload('1111');
      act(() => void fireEvent.mouseOver(screen.getByText('Uploading 1 file')));
      act(() => void fireEvent.click(screen.getByText('Click to cancel')));
      await startFileUpload('2222');
      expect(screen.queryByText('Uploading 1 file')).toBeInTheDocument();
    });

    it('shows upload errors', async () => {
      await startFileUpload('1111');
      await simulateUploadFailed('1111');
      expect(screen.getByText('1 file upload failed')).toBeInTheDocument();
      fireEvent.click(screen.getByTestId('dismiss-upload-errors'));
      expect(screen.queryByText('1 file upload failed')).not.toBeInTheDocument();
    });

    it('handles multiple placeholders', async () => {
      await startFileUpload('1111');
      expect(screen.getByText('Uploading 1 file')).toBeInTheDocument();
      await startFileUpload('2222');
      expect(screen.getByText('Uploading 2 files')).toBeInTheDocument();

      expect(descriptionTextContent()).toContain('Uploading #1');
      expect(descriptionTextContent()).toContain('Uploading #2');
      await simulateUploadSucceeded('1111');
      expect(descriptionTextContent()).not.toContain('Uploading #1');
      expect(descriptionTextContent()).toContain('Uploading #2');

      expect(descriptionTextContent()).toContain('https://image.example.com/1111');
      expect(descriptionTextContent()).not.toContain('https://image.example.com/2222');

      await simulateUploadSucceeded('2222');
      expect(descriptionTextContent()).not.toContain('Uploading #2');
      expect(descriptionTextContent()).toContain('https://image.example.com/2222');
    });

    it('inserts whitespace before inserted placeholder when necessary', async () => {
      act(() => {
        userEvent.type(getDescriptionEditor(), 'Hello!\n');
        //                                     ^ cursor
        getDescriptionEditor().selectionStart = 6;
        getDescriptionEditor().selectionEnd = 6;
      });
      await startFileUpload('1111');
      expect(descriptionTextContent()).toEqual('Hello! 【 Uploading #1 】\n');
      //                                       ^ inserted space  ^ no extra space
    });

    it('inserts whitespace after inserted placeholder when necessary', async () => {
      act(() => {
        userEvent.type(getDescriptionEditor(), 'Hello!\n');
        //                                          ^ cursor
        getDescriptionEditor().selectionStart = 0;
        getDescriptionEditor().selectionEnd = 0;
      });
      await startFileUpload('1111');
      expect(descriptionTextContent()).toEqual('【 Uploading #1 】 Hello!\n');
      //                                        ^ no space       ^ inserted space
    });

    it('preserves selection when setting placeholders', async () => {
      act(() => {
        userEvent.type(getDescriptionEditor(), 'Hello, world!\n');
        //                                            ^-----^ selection
        getDescriptionEditor().selectionStart = 2;
        getDescriptionEditor().selectionEnd = 8;
      });
      await startFileUpload('1111');
      expect(descriptionTextContent()).toEqual('He 【 Uploading #1 】 orld!\n');
      //                                          ^ inserted spaces ^

      // now cursor is after Uploading
      expect(getDescriptionEditor().selectionStart).toEqual(20);
      expect(getDescriptionEditor().selectionEnd).toEqual(20);
    });

    it('preserves selection when replacing placeholders', async () => {
      act(() => {
        userEvent.type(getDescriptionEditor(), 'fob\nbar\nbaz');
        //                                               ^ cursor
        getDescriptionEditor().selectionStart = 4;
        getDescriptionEditor().selectionEnd = 4;
      });
      await startFileUpload('1111');
      expect(descriptionTextContent()).toEqual('fob\n【 Uploading #1 】 bar\nbaz');
      //                     start new selection: ^--------------------------^
      getDescriptionEditor().selectionStart = 2;
      getDescriptionEditor().selectionEnd = 26;
      // make sure my indices are correct
      expect(descriptionTextContent()[getDescriptionEditor().selectionStart]).toEqual('b');
      expect(descriptionTextContent()[getDescriptionEditor().selectionEnd]).toEqual('a');

      await simulateUploadSucceeded('1111');
      expect(descriptionTextContent()).toEqual('fob\nhttps://image.example.com/1111 bar\nbaz');
      //                 selection is preserved:  ^---------------------------------------^

      // now cursor is after Uploading
      expect(getDescriptionEditor().selectionStart).toEqual(2);
      expect(getDescriptionEditor().selectionEnd).toEqual(40);
      expect(descriptionTextContent()[getDescriptionEditor().selectionStart]).toEqual('b');
      expect(descriptionTextContent()[getDescriptionEditor().selectionEnd]).toEqual('a');
    });

    describe('disable commit info view buttons while uploading', () => {
      beforeEach(() => {
        act(() => {
          simulateUncommittedChangedFiles({
            value: [{path: 'src/file1.js', status: 'M'}],
          });
        });
      });

      it('disables amend message button', async () => {
        CommitInfoTestUtils.clickToSelectCommit('b');
        CommitInfoTestUtils.clickToEditDescription();
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Amend Message'),
        ).not.toBeDisabled();
        await startFileUpload('1111');
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Amend Message'),
        ).toBeDisabled();
        await simulateUploadSucceeded('1111');
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Amend Message'),
        ).not.toBeDisabled();
      });

      it('disables amend button', async () => {
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Amend')).not.toBeDisabled();
        await startFileUpload('1111');
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Amend')).toBeDisabled();
        await simulateUploadSucceeded('1111');
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Amend')).not.toBeDisabled();
      });

      it('disables commit button', async () => {
        CommitInfoTestUtils.clickCommitMode();
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Commit')).not.toBeDisabled();
        await startFileUpload('1111');
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Commit')).toBeDisabled();
        await simulateUploadSucceeded('1111');
        expect(CommitInfoTestUtils.withinCommitActionBar().getByText('Commit')).not.toBeDisabled();
      });

      it('disables commit and submit button', async () => {
        act(() => {
          simulateMessageFromServer({
            type: 'repoInfo',
            info: {
              codeReviewSystem: {type: 'github'} as CodeReviewSystem,
              command: 'sl',
              repoRoot: '/repo',
              dotdir: '/repo/.sl',
              type: 'success',
              pullRequestDomain: undefined,
              preferredSubmitCommand: undefined,
            },
          });
        });
        CommitInfoTestUtils.clickCommitMode();
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Commit and Submit'),
        ).not.toBeDisabled();
        await startFileUpload('1111');
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Commit and Submit'),
        ).toBeDisabled();
        await simulateUploadSucceeded('1111');
        expect(
          CommitInfoTestUtils.withinCommitActionBar().getByText('Commit and Submit'),
        ).not.toBeDisabled();
      });
    });

    it('emits uploads to underlying store', async () => {
      CommitInfoTestUtils.clickCommitMode();
      act(() => {
        simulateUncommittedChangedFiles({value: [{path: 'foo.txt', status: 'M'}]});
      });
      act(() => {
        userEvent.type(CommitInfoTestUtils.getTitleEditor(), 'hi');
        userEvent.type(CommitInfoTestUtils.getDescriptionEditor(), 'hey\n');
      });
      await startFileUpload('1111');
      await simulateUploadSucceeded('1111');
      expect(descriptionTextContent()).toContain('https://image.example.com/1111');

      act(() => {
        fireEvent.click(CommitInfoTestUtils.withinCommitActionBar().getByText('Commit'));
      });
      await waitFor(() =>
        expectMessageSentToServer({
          type: 'runOperation',
          operation: expect.objectContaining({
            args: expect.arrayContaining([
              'commit',
              expect.stringMatching('hi\n+(Summary: )?hey\nhttps://image.example.com/1111'),
            ]),
          }),
        }),
      );
    });
  });
});
