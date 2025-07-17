/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Telemetry
{
    [TestClass]
    public class ScribeToolManagerTests
    {
        [TestMethod]
        public void Result_ToDebugString_FormatsCorrectly()
        {
            // Arrange
            var result = new Result
            {
                Stdout = "Standard output",
                Stderr = "Standard error",
                ExitCode = 1,
                Invocation = new Invocation("test.exe", "-arg1", "-arg2")
            };

            // Act
            var debugString = result.ToDebugString();

            // Assert
            Assert.IsTrue(debugString.Contains("test.exe -arg1 -arg2"));
            Assert.IsTrue(debugString.Contains("exit code 1"));
            Assert.IsTrue(debugString.Contains("stdout:\nStandard output"));
            Assert.IsTrue(debugString.Contains("stderr:\nStandard error"));
        }

        [TestMethod]
        public void Invocation_Constructor_WithEnumerable_SetsProperties()
        {
            // Arrange
            var fileName = "test.exe";
            var arguments = new List<string> { "-arg1", "-arg2" };

            // Act
            var invocation = new Invocation(fileName, arguments);

            // Assert
            Assert.AreEqual(fileName, invocation.FileName);
            CollectionAssert.AreEqual(arguments, invocation.Arguments.ToList());
        }

        [TestMethod]
        public void Invocation_Constructor_WithParams_SetsProperties()
        {
            // Arrange
            var fileName = "test.exe";
            var arguments = new[] { "-arg1", "-arg2" };

            // Act
            var invocation = new Invocation(fileName, arguments);

            // Assert
            Assert.AreEqual(fileName, invocation.FileName);
            CollectionAssert.AreEqual(arguments, invocation.Arguments.ToList());
        }

        [TestMethod]
        public void Invocation_ArgumentString_JoinsArguments()
        {
            // Arrange
            var invocation = new Invocation("test.exe", "-arg1", "-arg2");

            // Act
            var argumentString = invocation.ArgumentString;

            // Assert
            Assert.AreEqual("-arg1 -arg2", argumentString);
        }

        [TestMethod]
        public void Invocation_ArgumentString_ReturnsEmptyStringWhenNoArguments()
        {
            // Arrange
            var invocation = new Invocation("test.exe", (IEnumerable<string>)null);

            // Act
            var argumentString = invocation.ArgumentString;

            // Assert
            Assert.AreEqual(string.Empty, argumentString);
        }

        [TestMethod]
        public void Invocation_HasArguments_ReturnsTrueWhenArgumentsExist()
        {
            // Arrange
            var invocation = new Invocation("test.exe", "-arg1", "-arg2");

            // Act & Assert
            Assert.IsTrue(invocation.HasArguments);
        }

        [TestMethod]
        public void Invocation_HasArguments_ReturnsFalseWhenNoArguments()
        {
            // Arrange
            var invocation = new Invocation("test.exe", (IEnumerable<string>)null);

            // Act & Assert
            Assert.IsFalse(invocation.HasArguments);
        }

        [TestMethod]
        public void Invocation_ToString_IncludesFileNameAndArguments()
        {
            // Arrange
            var invocation = new Invocation("test.exe", "-arg1", "-arg2");

            // Act
            var toString = invocation.ToString();

            // Assert
            Assert.AreEqual("test.exe -arg1 -arg2", toString);
        }

        [TestMethod]
        public void Invocation_ToString_OnlyIncludesFileNameWhenNoArguments()
        {
            // Arrange
            var invocation = new Invocation("test.exe", (IEnumerable<string>)null);

            // Act
            var toString = invocation.ToString();

            // Assert
            Assert.AreEqual("test.exe", toString);
        }

        [TestMethod]
        public void ScribeToolManager_Execute_WithSimpleCommand()
        {
            // This test will actually execute a command, so we'll use a simple command that should work on most systems
            string command = Environment.OSVersion.Platform == PlatformID.Win32NT ? "cmd.exe" : "/bin/echo";
            string[] args = Environment.OSVersion.Platform == PlatformID.Win32NT ? new[] { "/c", "echo", "test" } : new[] { "test" };

            // Act
            var result = ScribeToolManager.Execute(command, args);

            // Assert
            Assert.AreEqual(0, result.ExitCode);
            Assert.IsTrue(result.Stdout.Contains("test"));
        }

        [TestMethod]
        public void ScribeToolManager_Execute_WithInvocation()
        {
            // This test will actually execute a command, so we'll use a simple command that should work on most systems
            string command = Environment.OSVersion.Platform == PlatformID.Win32NT ? "cmd.exe" : "/bin/echo";
            string[] args = Environment.OSVersion.Platform == PlatformID.Win32NT ? new[] { "/c", "echo", "test" } : new[] { "test" };

            var invocation = new Invocation(command, args);

            // Act
            var result = ScribeToolManager.Execute(invocation);

            // Assert
            Assert.AreEqual(0, result.ExitCode);
            Assert.IsTrue(result.Stdout.Contains("test"));
        }
    }
}
