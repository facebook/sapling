/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

using System;
using System.ComponentModel;
using System.Drawing.Design;
using System.Reflection;
using Microsoft.VisualStudio.TestTools.UnitTesting;

namespace InteractiveSmartlogVSExtension.Tests.Options
{
    [TestClass]
    public class OptionsPageTests
    {
        [TestMethod]
        public void DiffTool_HasCategoryAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("DiffTool");

            // Act
            var attribute = property.GetCustomAttribute<CategoryAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Diff", attribute.Category);
        }

        [TestMethod]
        public void DiffTool_HasDisplayNameAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("DiffTool");

            // Act
            var attribute = property.GetCustomAttribute<DisplayNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Diff Tool", attribute.DisplayName);
        }

        [TestMethod]
        public void DiffTool_HasDescriptionAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("DiffTool");

            // Act
            var attribute = property.GetCustomAttribute<System.ComponentModel.DescriptionAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Select your choice of diff tool.", attribute.Description);
        }

        [TestMethod]
        public void DiffTool_HasTypeConverterAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("DiffTool");

            // Act
            var attribute = property.GetCustomAttribute<TypeConverterAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            // Use Contains instead of AreEqual to handle assembly version differences
            Assert.IsTrue(attribute.ConverterTypeName.Contains("InteractiveSmartlogVSExtension.DiffToolConverter"));
        }

        // Removed DiffTool_DefaultValue_IsVisualStudio test because it requires instantiating OptionsPage

        [TestMethod]
        public void CustomDiffToolExe_HasCategoryAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolExe");

            // Act
            var attribute = property.GetCustomAttribute<CategoryAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Diff: Custom Diff Tool", attribute.Category);
        }

        [TestMethod]
        public void CustomDiffToolExe_HasDisplayNameAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolExe");

            // Act
            var attribute = property.GetCustomAttribute<DisplayNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Custom Diff Tool", attribute.DisplayName);
        }

        [TestMethod]
        public void CustomDiffToolExe_HasDescriptionAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolExe");

            // Act
            var attribute = property.GetCustomAttribute<System.ComponentModel.DescriptionAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("If using a custom diff tool, specify the executable path here.", attribute.Description);
        }

        [TestMethod]
        public void CustomDiffToolExe_HasEditorAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolExe");

            // Act
            var attribute = property.GetCustomAttribute<EditorAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            // Use Contains instead of AreEqual to handle assembly version differences
            Assert.IsTrue(attribute.EditorTypeName.Contains("InteractiveSmartlogVSExtension.FilePickerEditor"));
            Assert.IsTrue(attribute.EditorBaseTypeName.Contains("System.Drawing.Design.UITypeEditor"));
        }

        // Removed CustomDiffToolExe_DefaultValue_IsEmptyString test because it requires instantiating OptionsPage

        [TestMethod]
        public void CustomDiffToolArgs_HasCategoryAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolArgs");

            // Act
            var attribute = property.GetCustomAttribute<CategoryAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Diff: Custom Diff Tool", attribute.Category);
        }

        [TestMethod]
        public void CustomDiffToolArgs_HasDisplayNameAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolArgs");

            // Act
            var attribute = property.GetCustomAttribute<DisplayNameAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.AreEqual("Custom Diff Tool Args", attribute.DisplayName);
        }

        [TestMethod]
        public void CustomDiffToolArgs_HasDescriptionAttribute()
        {
            // Arrange
            var property = typeof(OptionsPage).GetProperty("CustomDiffToolArgs");

            // Act
            var attribute = property.GetCustomAttribute<System.ComponentModel.DescriptionAttribute>();

            // Assert
            Assert.IsNotNull(attribute);
            Assert.IsTrue(attribute.Description.Contains("If using a custom diff tool, specify the arguments for it here."));
            Assert.IsTrue(attribute.Description.Contains("%bf : base filename"));
            Assert.IsTrue(attribute.Description.Contains("Example for p4merge: -nl %bn -nr %wn %bf %wf"));
        }

        // Removed CustomDiffToolArgs_DefaultValue_IsEmptyString test because it requires instantiating OptionsPage
        // Removed Properties_SetAndGetValues test because it requires instantiating OptionsPage
    }
}
