/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Reflection;
using System.Runtime.Serialization;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Models
{
    [TestClass]
    public class CommandExecutionResultTests
    {
        [TestMethod]
        public void CommandExecutionResult_HasDataContractAttribute()
        {
            // Arrange
            var type = typeof(CommandExecutionResult);

            // Act
            var attribute = type.GetCustomAttribute<DataContractAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Url_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("Url");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Port_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("Port");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Token_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("Token");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Pid_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("Pid");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void WasServerReused_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("WasServerReused");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void LogFileLocation_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("LogFileLocation");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Command_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(CommandExecutionResult).GetProperty("Command");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Properties_SetAndGetValues()
        {
            // Arrange
            var result = new CommandExecutionResult();
            var url = "http://localhost:8080";
            var port = "8080";
            var token = "abc123";
            var pid = "12345";
            var wasServerReused = true;
            var logFileLocation = "C:\\logs\\log.txt";
            var command = "sl web";

            // Act
            result.Url = url;
            result.Port = port;
            result.Token = token;
            result.Pid = pid;
            result.WasServerReused = wasServerReused;
            result.LogFileLocation = logFileLocation;
            result.Command = command;

            // Assert
            Assert.AreEqual(url, result.Url);
            Assert.AreEqual(port, result.Port);
            Assert.AreEqual(token, result.Token);
            Assert.AreEqual(pid, result.Pid);
            Assert.AreEqual(wasServerReused, result.WasServerReused);
            Assert.AreEqual(logFileLocation, result.LogFileLocation);
            Assert.AreEqual(command, result.Command);
        }
    }
}
