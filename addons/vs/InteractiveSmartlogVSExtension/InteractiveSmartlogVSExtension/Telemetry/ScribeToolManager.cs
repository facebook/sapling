/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System.Collections.Generic;
using System.Diagnostics;


namespace InteractiveSmartlogVSExtension
{

    public struct Result
    {
        public string Stdout;
        public string Stderr;
        public int ExitCode;
        public Invocation Invocation;

        public string ToDebugString()
        {
            return $"\"{Invocation}\" returned exit code {ExitCode}\nstdout:\n{Stdout}\nstderr:\n{Stderr}";
        }
    }

    public readonly struct Invocation
    {
        public string FileName { get; }

        public IEnumerable<string> Arguments { get; }

        public string ArgumentString => Arguments != null
          ? string.Join(" ", Arguments)
          : string.Empty;

        public bool HasArguments => Arguments != null;

        public override string ToString() =>
          HasArguments
            ? $"{FileName} {ArgumentString}"
            : FileName;

        public Invocation(string fileName, IEnumerable<string> arguments) => (FileName, Arguments) = (fileName, arguments);

        public Invocation(string fileName, params string[] arguments) => (FileName, Arguments) = (fileName, arguments);
    }


    internal class ScribeToolManager
    {
        public static Result Execute(string filename, params string[] arguments) =>
          Execute(new Invocation(filename, arguments));

        public static Result Execute(Invocation invocation)
        {
            Process process = StartProcess(invocation);
            // @lint-ignore UNITYBANNEDAPI - not Quest not Unity.
            process.WaitForExit();

            var stdout = process.StandardOutput.ReadToEnd();
            var stderr = process.StandardError.ReadToEnd();

            return new Result
            {
                Stdout = stdout,
                Stderr = stderr,
                ExitCode = process.ExitCode,
                Invocation = invocation
            };
        }

        private static Process StartProcess(Invocation invocation)
        {
            Process process = new Process
            {
                StartInfo = GetStartInfo(invocation)
            };

            process.Start();
            return process;
        }

        static ProcessStartInfo GetStartInfo(Invocation invocation) =>
          new ProcessStartInfo
          {
              FileName = invocation.FileName,
              Arguments = invocation.ArgumentString,
              RedirectStandardOutput = true,
              RedirectStandardError = true,
              UseShellExecute = false,
              CreateNoWindow = true
          };
    }
}
