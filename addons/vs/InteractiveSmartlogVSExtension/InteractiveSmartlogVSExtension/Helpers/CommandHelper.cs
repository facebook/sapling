/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using EnvDTE;
using InteractiveSmartlogVSExtension.Enums;
using InteractiveSmartlogVSExtension.Helpers;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Threading;
using Task = System.Threading.Tasks.Task;

namespace InteractiveSmartlogVSExtension
{

    class CommandHelper
    {
        private class TempFile : IDisposable
        {
            public TempFile()
            {
                Path = System.IO.Path.GetTempFileName();
            }

            public void Dispose()
            {
                File.Delete(Path);
            }

            public string Path { get; }
        }

        public static async Task ShowDiffAsync(InteractiveSmartlogVSExtensionPackage package, DiffType diffType, Document document)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);
            if (document == null)
                return;

            var path = document.FullName;
            if (String.IsNullOrEmpty(path))
                return;

            bool isDirty = !document.Saved;

            if (package.DiffTool == DiffTool.VisualStudio)
            {
                await ShowInternalDiffAsync(package, diffType, path, isDirty).ConfigureAwait(false);
            }
            else
            {
                string documentText = null;
                if (isDirty)
                {
                    var textDocument = document.Object("TextDocument") as TextDocument;
                    if (textDocument == null)
                    {
                        throw new Exception("Failed to get TextDocument object from active document.");
                    }

                    // Get the full text of the document
                    EditPoint startPoint = textDocument.StartPoint.CreateEditPoint();
                    documentText = startPoint.GetText(textDocument.EndPoint);
                }

                var (diffToolExe, diffToolArgs) = GetDiffCommand(package);
                if (String.IsNullOrEmpty(diffToolExe) || String.IsNullOrEmpty(diffToolArgs))
                {
                    throw new Exception(
                        "To use a custom diff tool, you need to specify a tool executable and argument string.\n\n" +
                        "Please setup at Tools -> Options -> InteractiveSmartlog.");
                }
                package.JoinableTaskFactory.RunAsync(async () => await ShowExternalDiffAsync(package, diffToolExe, diffToolArgs, diffType, path, documentText));
            }
        }

        public static async Task RevertChangesAsync(AsyncPackage package, string path)
        {
            try
            {
                // switch to worker thread
                await TaskScheduler.Default;

                var workingDir = Path.GetDirectoryName(path);
                var filename = Path.GetFileName(path);
                var arguments = $"revert \"{filename}\"";
                var cancellationTokenSource = new CancellationTokenSource(TimeSpan.FromMilliseconds(Constants.OverallSlOperationTimeoutMs));
                await RetryHelper.RetryWithExponentialBackoffAsync(
                    () => RunAndGetOutputWithTimeoutAsync(
                                "sl.exe",
                                arguments,
                                workingDir,
                                cancellationTokenSource.Token),
                    maxAttempts: Constants.MaxSlOperationRetries,
                    initialDelayMs: Constants.SlOperationRetryDelayMs
                );
                await CommonHelper.LogSuccessAsync(ActionType.RevertDiffChanges);
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(package, ActionType.RevertDiffChanges, ErrorCodes.SlCatFailed, $"Revert failed for path: {path}, error: {ex.Message}");
                throw;
            }
        }

        public static async Task ShowInternalDiffAsync(InteractiveSmartlogVSExtensionPackage package, DiffType diffType, string path, bool documentDirty)
        {
            TempFile tempFile = null;
            try
            {
                // switch to worker thread
                await TaskScheduler.Default;

                var (revision, revisionDesc, diffDesc) = GetDiffStrings(diffType);

                var workingDir = Path.GetDirectoryName(path);
                var filename = Path.GetFileName(path);
                var cancellationTokenSource = new CancellationTokenSource(TimeSpan.FromMilliseconds(Constants.OverallSlOperationTimeoutMs));
                string baseDocumentText = await RetryHelper.RetryWithExponentialBackoffAsync(
                    () => GetDocumentFromRepoAsync(ActionType.OpenInternalDiffView, diffType, filename, workingDir, cancellationTokenSource.Token),
                    maxAttempts: Constants.MaxSlOperationRetries,
                    initialDelayMs: Constants.SlOperationRetryDelayMs
                );

                using (tempFile = new TempFile())
                {
                    var tempFilename = tempFile.Path;
                    File.WriteAllText(tempFilename, baseDocumentText);
                    string leftFileMoniker = tempFilename;
                    string rightFileMoniker = path;

                    string decoratedFilename = $"{filename}{(documentDirty ? "*" : "")}";

                    string caption = $"DIFF - {decoratedFilename} {diffDesc}";

                    string tooltip = null;

                    string leftLabel = $"{filename} ({revisionDesc}";
                    string rightLabel = $"{decoratedFilename} (Working Copy)";
                    string inlineLabel = $"{leftLabel} -> {rightLabel}";
                    string roles = null;

                    await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);
                    IVsDifferenceService differenceService = await package.GetServiceAsync(typeof(SVsDifferenceService)) as IVsDifferenceService;
                    if (differenceService == null)
                        return;

                    __VSDIFFSERVICEOPTIONS grfDiffOptions = __VSDIFFSERVICEOPTIONS.VSDIFFOPT_LeftFileIsTemporary;
                    differenceService.OpenComparisonWindow2(leftFileMoniker, rightFileMoniker, caption, tooltip, leftLabel, rightLabel, inlineLabel, roles, (uint)grfDiffOptions);
                    await CommonHelper.LogSuccessAsync(ActionType.OpenInternalDiffView);
                }
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(package, Enums.ActionType.OpenInternalDiffView, ErrorCodes.SlCatFailed, $"Internal diff failed for path: {path}, error: {ex.Message}");
                throw;
            }
            finally
            {
                tempFile?.Dispose(); // Ensures cleanup even if an exception occurs
            }
        }

        private static async Task ShowExternalDiffAsync(AsyncPackage package,
    string diffToolExe, string diffToolArgs, DiffType diffType, string workingPath, string documentText)
        {
            TempFile tempFile = null;
            try
            {
                await TaskScheduler.Default;

                var (revision, revisionDesc, diffDesc) = GetDiffStrings(diffType);
                var workingDir = Path.GetDirectoryName(workingPath);
                var filename = Path.GetFileName(workingPath);

                var cancellationTokenSource = new CancellationTokenSource(TimeSpan.FromMilliseconds(Constants.OverallSlOperationTimeoutMs));
                string baseDocumentText = await RetryHelper.RetryWithExponentialBackoffAsync(
                    () => GetDocumentFromRepoAsync(ActionType.OpenExternalDiffView, diffType, filename, workingDir, cancellationTokenSource.Token),
                    maxAttempts: Constants.MaxSlOperationRetries,
                    initialDelayMs: Constants.SlOperationRetryDelayMs
                );

                using (var tempBaseFile = new TempFile())
                {
                    var basePath = tempBaseFile.Path;
                    File.WriteAllText(basePath, baseDocumentText);

                    TempFile tempWorkingFile = documentText == null ? null : new TempFile();
                    using (tempWorkingFile)
                    {
                        if (documentText != null)
                        {
                            File.WriteAllText(tempWorkingFile.Path, documentText);
                            workingPath = tempWorkingFile.Path;
                        }

                        bool documentDirty = documentText != null;
                        string decoratedFilename = $"{filename}{(documentDirty ? "*" : "")}";

                        diffToolArgs = diffToolArgs.Replace("%bf", $"\"{basePath}\"");
                        diffToolArgs = diffToolArgs.Replace("%wf", $"\"{workingPath}\"");
                        diffToolArgs = diffToolArgs.Replace("%bn", $"\"{filename} ({revisionDesc})\"");
                        diffToolArgs = diffToolArgs.Replace("%wn", $"\"{decoratedFilename} (Working Copy)\"");

                        System.Diagnostics.Process.Start(new ProcessStartInfo
                        {
                            FileName = diffToolExe,
                            Arguments = diffToolArgs,
                            UseShellExecute = false,
                            CreateNoWindow = true
                        });

                        await CommonHelper.LogSuccessAsync(ActionType.OpenExternalDiffView);

                        // Give our tool a little time to actually launch and read the temporary file before we try to delete it.
                        // This is fine since we've scheduled the task on a worker thread.
                        System.Threading.Thread.Sleep(5000);
                    }
                }
            }
            catch (Win32Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(package, Enums.ActionType.OpenExternalDiffView, ErrorCodes.SlCatFailed, $"Diff tool launch failed: {ex.Message}");
                if (ex.ErrorCode == -2147467259)
                {
                    throw new Exception($"Could not find '{diffToolExe}'.  Make sure it is installed and in your PATH.");
                }
                throw;
            }
            catch (Exception ex)
            {
                await CommonHelper.LogAndHandleErrorAsync(package, ActionType.OpenExternalDiffView, ErrorCodes.SlCatFailed, $"External diff failed for path: {workingPath}, error: {ex.Message}");
                throw;
            }
            finally
            {
                tempFile?.Dispose(); // Ensures cleanup even if an exception occurs
            }
        }

        private static (string exe, string args) GetDiffCommand(InteractiveSmartlogVSExtensionPackage package)
        {
            switch (package.DiffTool)
            {
                case DiffTool.p4merge:
                    return ("p4merge.exe", "-nl %bn -nr %wn %bf %wf");
                case DiffTool.WinMerge:
                    return ("WinMergeU.exe", "/r /e /u /dl %bn /dr %wn %bf %wf");
                case DiffTool.BeyondCompare:
                    return ("BComp.exe", "%bf %wf /title1=%bn /title2=%wn /readonly");
                case DiffTool.Custom:
                default:
                    return (package.CustomDiffToolExe, package.CustomDiffToolArgs);
            }
        }

        private static (string revision, string revisionDesc, string diffDesc) GetDiffStrings(DiffType diffType)
        {
            switch (diffType)
            {
                case DiffType.UNCOMMITTED:
                default:
                    return (".", "HEAD", "Uncommitted Changes");
                case DiffType.HEAD:
                    return (".^", "PARENT", "Head Changes");
                case DiffType.STACK:
                    return ("bottom^", "TRUNK", "Stack Changes");
            }
        }

        private static async Task<string> GetDocumentFromRepoAsync(
            ActionType action,
            DiffType diffType,
            string filename,
            string workingDir,
            CancellationToken cancellationToken = default)
        {
            int attempt = 0;
            int delay = Constants.SlOperationRetryDelayMs;
            Exception lastException = null;

            while (attempt < Constants.MaxSlOperationRetries)
            {
                try
                {
                    var (revision, revisionDesc, diffDesc) = GetDiffStrings(diffType);
                    var arguments = $"cat -r {revision} \"{filename}\"";
                    var result = await RunAndGetOutputWithTimeoutAsync(
                        "sl.exe",
                        arguments,
                        workingDir,
                        cancellationToken
                    );

                    if (result.code != 0)
                    {
                        if (diffType == DiffType.STACK && result.stderr.Contains("current commit is public"))
                        {
                            return await GetDocumentFromRepoAsync(action, DiffType.UNCOMMITTED, filename, workingDir, cancellationToken);
                        }
                        throw new Exception($"sl {arguments} returned exit code {result.code}\nerror: {result.stderr.Trim()}\noutput: {result.stdout.Trim()}");
                    }
                    return result.stdout;
                }
                catch (OperationCanceledException)
                {
                    LoggingHelper.WriteToDebug($"Cancelled on attempt {attempt + 1} for file: {filename}");
                    throw;
                }
                catch (Exception ex)
                {
                    lastException = ex;
                    LoggingHelper.WriteToDebug($"Attempt {attempt + 1}: {ex.Message}");
                    await Task.Delay(delay, cancellationToken);
                    delay *= 2; // Exponential backoff
                    attempt++;
                }
            }
            throw new Exception("Failed to get document from repo after retries.", lastException);
        }

        /// <summary>
        /// Reloads the ISL tool window
        /// </summary>
        /// <param name="package"></param>
        public static async Task ReloadISLToolWindowAsync(InteractiveSmartlogVSExtensionPackage package)
        {
            ToolWindowPane window = package.FindToolWindow(typeof(InteractiveSmartlogToolWindow), 0, true);
            InteractiveSmartlogToolWindowControl control = window?.Content as InteractiveSmartlogToolWindowControl;
            await control?.ReloadAsync();
        }

        // @lint-ignore-every UNITYBANNEDAPI - This is not Unity code but a VSIX project
        private static async Task<(int code, string stdout, string stderr)> RunAndGetOutputWithTimeoutAsync(
            string command,
            string arguments,
            string workingDirectory,
            CancellationToken cancellationToken)
        {
            var processInfo = new ProcessStartInfo
            {
                FileName = command,
                Arguments = arguments,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardError = true,
                RedirectStandardOutput = true,
                WorkingDirectory = workingDirectory,
            };

            using (var process = System.Diagnostics.Process.Start(processInfo) ?? throw new Exception("Process.Start returned null"))
            {
                var stdOutTask = process.StandardOutput.ReadToEndAsync();
                var stdErrTask = process.StandardError.ReadToEndAsync();

                var waitForExitTask = Task.Run(() => process.WaitForExit(), cancellationToken);
                var timeoutTask = Task.Delay(Constants.SlOperationProcessTimeoutMs, cancellationToken);

                var completedTask = await Task.WhenAny(waitForExitTask, timeoutTask).ConfigureAwait(false);

                if (cancellationToken.IsCancellationRequested)
                {
                    try { process.Kill(); } catch { }
                    throw new OperationCanceledException("Operation was cancelled.", cancellationToken);
                }

                if (completedTask == timeoutTask)
                {
                    try { process.Kill(); } catch { }
                    throw new TimeoutException($"Timeout running '{command} {arguments}' in '{workingDirectory}'");
                }

                string stdout = await stdOutTask.ConfigureAwait(false);
                string stderr = await stdErrTask.ConfigureAwait(false);

                return (process.ExitCode, stdout, stderr);
            }
        }
    }
}
