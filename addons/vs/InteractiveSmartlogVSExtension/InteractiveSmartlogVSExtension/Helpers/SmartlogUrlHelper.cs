/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Threading;
using Newtonsoft.Json;

namespace InteractiveSmartlogVSExtension.Helpers
{
    public static class SmartlogUrlHelper
    {
        public static async Task<string> ComputeSlWebUrlStaticAsync(CancellationToken cancellationToken = default)
        {
            // Prepare sl web arguments
            string[] args = GetSlWebArguments();

            var startInfo = new ProcessStartInfo
            {
                FileName = "sl.exe",
                Arguments = string.Join(" ", args),
                WorkingDirectory = GetSolutionDirectory(),
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };

            // Check if sl.exe is accessible
            if (!File.Exists(Path.Combine(startInfo.WorkingDirectory, "sl.exe")) &&
                string.IsNullOrEmpty(Environment.GetEnvironmentVariable("PATH")) ||
                !Environment.GetEnvironmentVariable("PATH").Split(';').Any(p => File.Exists(Path.Combine(p, "sl.exe"))))
            {
                throw new FileNotFoundException("sl.exe not found in PATH or working directory.");
            }

            using (var process = new Process { StartInfo = startInfo })
            {
                process.Start();
                var stdOutTask = process.StandardOutput.ReadToEndAsync();
                var stdErrTask = process.StandardError.ReadToEndAsync();

                var timeoutTask = Task.Delay(Constants.SlOperationProcessTimeoutMs);
                await Task.WhenAny(process.WaitForExitAsync(cancellationToken), timeoutTask).ConfigureAwait(false);

                string output = await stdOutTask.ConfigureAwait(false);
                string errorOutput = await stdErrTask.ConfigureAwait(false);

                // Check the exit code
                if (process.ExitCode != 0)
                {
                    throw new Exception($"sl web command failed with ExitCode: {process.ExitCode} . Error: {errorOutput}");
                }

                if (string.IsNullOrWhiteSpace(output))
                {
                    throw new Exception("sl web returned empty output.");
                }

                CommandExecutionResult result;
                try
                {
                    result = JsonConvert.DeserializeObject<CommandExecutionResult>(output);
                }
                catch (JsonException ex)
                {
                    throw new Exception($"Failed to deserialize sl command output. {ex.Message}");
                }

                if (result == null || string.IsNullOrEmpty(result.Url) || !Uri.IsWellFormedUriString(result.Url, UriKind.Absolute))
                {
                    throw new Exception("sl web did not return a valid URL.");
                }

                return result.Url;
            }
        }

        private static string GetSolutionDirectory()
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            var solutionService = (IVsSolution)Package.GetGlobalService(typeof(SVsSolution));
            solutionService.GetSolutionInfo(out string solutionDir, out _, out _);
            return solutionDir ?? Directory.GetCurrentDirectory();
        }

        /// <summary>
        /// Gets the sl web arguments.
        /// </summary>
        /// <param name="usePlatformArgs"></param>
        /// <returns></returns>
        private static string[] GetSlWebArguments()
        {
            string windowId = CommonHelper.WindowId.Value;
            string[] args = new string[]
            {
                "web",
                "--json",
                "--no-open",
                $"--session {windowId}",
                "--platform visualStudio"
            };
            return args;
        }

        /// <summary>
        /// Checks if a timestamp is within a certain number of minutes of the current time.
        /// </summary>
        /// <param name="timestamp"></param>
        /// <returns></returns>
        public static bool IsCachedUrlFresh(DateTime timestamp)
        {
            return (DateTime.Now - timestamp).TotalMinutes <= Constants.StalenessThresholdMinutes;
        }
    }
}
