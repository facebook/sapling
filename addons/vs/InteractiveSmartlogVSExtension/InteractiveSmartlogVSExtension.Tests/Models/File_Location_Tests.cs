/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Models
{
    [TestClass]
    public class FileLocationTests
    {
        [TestMethod]
        public void FileLocation_HasDataContractAttribute()
        {
            // Arrange
            var type = typeof(FileLocation);

            // Act
            var attribute = type.GetCustomAttribute<DataContractAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void FilePath_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("FilePath");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void FilePath_HasJsonPropertyNameAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("FilePath");

            // Act
            var attribute = property.GetCustomAttribute<JsonPropertyNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("filePath", attribute.Name);
        }

        [TestMethod]
        public void Line_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("Line");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Line_HasJsonPropertyNameAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("Line");

            // Act
            var attribute = property.GetCustomAttribute<JsonPropertyNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("line", attribute.Name);
        }

        [TestMethod]
        public void Col_HasDataMemberAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("Col");

            // Act
            var attribute = property.GetCustomAttribute<DataMemberAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
        }

        [TestMethod]
        public void Col_HasJsonPropertyNameAttribute()
        {
            // Arrange
            var property = typeof(FileLocation).GetProperty("Col");

            // Act
            var attribute = property.GetCustomAttribute<JsonPropertyNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("col", attribute.Name);
        }

        [TestMethod]
        public void Properties_SetAndGetValues()
        {
            // Arrange
            var fileLocation = new FileLocation();
            var filePath = "C:\\test\\file.cs";
            var line = 42;
            var col = 10;

            // Act
            fileLocation.FilePath = filePath;
            fileLocation.Line = line;
            fileLocation.Col = col;

            // Assert
            Assert.AreEqual(filePath, fileLocation.FilePath);
            Assert.AreEqual(line, fileLocation.Line);
            Assert.AreEqual(col, fileLocation.Col);
        }

        [TestMethod]
        public void JsonSerialization_UsesJsonPropertyNames()
        {
            // Arrange
            var fileLocation = new FileLocation
            {
                FilePath = "C:\\test\\file.cs",
                Line = 42,
                Col = 10
            };

            // Act
            var json = JsonSerializer.Serialize(fileLocation);

            // Assert
            Assert.IsTrue(json.Contains("\"filePath\""));
            Assert.IsTrue(json.Contains("\"line\""));
            Assert.IsTrue(json.Contains("\"col\""));
        }

        [TestMethod]
        public void JsonDeserialization_UsesJsonPropertyNames()
        {
            // Arrange
            var json = "{\"filePath\":\"C:\\\\test\\\\file.cs\",\"line\":42,\"col\":10}";

            // Act
            var fileLocation = JsonSerializer.Deserialize<FileLocation>(json);

            // Assert
            Assert.IsNotNull(fileLocation);
            Assert.AreEqual("C:\\test\\file.cs", fileLocation.FilePath);
            Assert.AreEqual(42, fileLocation.Line);
            Assert.AreEqual(10, fileLocation.Col);
        }
    }
}
