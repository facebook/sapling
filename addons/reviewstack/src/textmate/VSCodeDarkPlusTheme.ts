/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {IRawTheme} from 'vscode-textmate';

// Theme data fetched from:
// https://github.com/microsoft/vscode/raw/a716714a88891cad69c0753fb95923870df295f5/extensions/theme-defaults/themes/dark_plus.json

const theme: IRawTheme = {
  name: 'Dark+ (default dark)',
  settings: [
    {
      settings: {
        foreground: '#D4D4D4',
        background: '#1E1E1E',
      },
    },
    {
      name: 'Function declarations',
      scope: [
        'entity.name.function',
        'support.function',
        'support.constant.handlebars',
        'source.powershell variable.other.member',
        'entity.name.operator.custom-literal',
      ],
      settings: {
        foreground: '#DCDCAA',
      },
    },
    {
      name: 'Types declaration and references',
      scope: [
        'meta.return-type',
        'support.class',
        'support.type',
        'entity.name.type',
        'entity.name.namespace',
        'entity.other.attribute',
        'entity.name.scope-resolution',
        'entity.name.class',
        'storage.type.numeric.go',
        'storage.type.byte.go',
        'storage.type.boolean.go',
        'storage.type.string.go',
        'storage.type.uintptr.go',
        'storage.type.error.go',
        'storage.type.rune.go',
        'storage.type.cs',
        'storage.type.generic.cs',
        'storage.type.modifier.cs',
        'storage.type.variable.cs',
        'storage.type.annotation.java',
        'storage.type.generic.java',
        'storage.type.java',
        'storage.type.object.array.java',
        'storage.type.primitive.array.java',
        'storage.type.primitive.java',
        'storage.type.token.java',
        'storage.type.groovy',
        'storage.type.annotation.groovy',
        'storage.type.parameters.groovy',
        'storage.type.generic.groovy',
        'storage.type.object.array.groovy',
        'storage.type.primitive.array.groovy',
        'storage.type.primitive.groovy',
      ],
      settings: {
        foreground: '#4EC9B0',
      },
    },
    {
      name: 'Types declaration and references, TS grammar specific',
      scope: [
        'meta.type.cast.expr',
        'meta.type.new.expr',
        'support.constant.math',
        'support.constant.dom',
        'support.constant.json',
        'entity.other.inherited-class',
      ],
      settings: {
        foreground: '#4EC9B0',
      },
    },
    {
      name: 'Control flow / Special keywords',
      scope: [
        'keyword.control',
        'source.cpp keyword.operator.new',
        'keyword.operator.delete',
        'keyword.other.using',
        'keyword.other.operator',
        'entity.name.operator',
      ],
      settings: {
        foreground: '#C586C0',
      },
    },
    {
      name: 'Variable and parameter name',
      scope: [
        'variable',
        'meta.definition.variable.name',
        'support.variable',
        'entity.name.variable',
      ],
      settings: {
        foreground: '#9CDCFE',
      },
    },
    {
      name: 'Constants and enums',
      scope: ['variable.other.constant', 'variable.other.enummember'],
      settings: {
        foreground: '#51B6C4',
      },
    },
    {
      name: 'Object keys, TS grammar specific',
      scope: 'meta.object-literal.key',
      settings: {
        foreground: '#9CDCFE',
      },
    },
    {
      name: 'CSS property value',
      scope: [
        'support.constant.property-value',
        'support.constant.font-name',
        'support.constant.media-type',
        'support.constant.media',
        'constant.other.color.rgb-value',
        'constant.other.rgb-value',
        'support.constant.color',
      ],
      settings: {
        foreground: '#CE9178',
      },
    },
    {
      name: 'Regular expression groups',
      scope: [
        'punctuation.definition.group.regexp',
        'punctuation.definition.group.assertion.regexp',
        'punctuation.definition.character-class.regexp',
        'punctuation.character.set.begin.regexp',
        'punctuation.character.set.end.regexp',
        'keyword.operator.negation.regexp',
        'support.other.parenthesis.regexp',
      ],
      settings: {
        foreground: '#CE9178',
      },
    },
    {
      scope: [
        'constant.character.character-class.regexp',
        'constant.other.character-class.set.regexp',
        'constant.other.character-class.regexp',
        'constant.character.set.regexp',
      ],
      settings: {
        foreground: '#d16969',
      },
    },
    {
      scope: ['keyword.operator.or.regexp', 'keyword.control.anchor.regexp'],
      settings: {
        foreground: '#DCDCAA',
      },
    },
    {
      scope: 'keyword.operator.quantifier.regexp',
      settings: {
        foreground: '#d7ba7d',
      },
    },
    {
      scope: 'constant.character',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'constant.character.escape',
      settings: {
        foreground: '#d7ba7d',
      },
    },
    {
      scope: 'entity.name.label',
      settings: {
        foreground: '#C8C8C8',
      },
    },
    {
      scope: ['meta.embedded', 'source.groovy.embedded'],
      settings: {
        foreground: '#D4D4D4',
      },
    },
    {
      scope: 'emphasis',
      settings: {
        fontStyle: 'italic',
      },
    },
    {
      scope: 'strong',
      settings: {
        fontStyle: 'bold',
      },
    },
    {
      scope: 'header',
      settings: {
        foreground: '#000080',
      },
    },
    {
      scope: 'comment',
      settings: {
        foreground: '#6A9955',
      },
    },
    {
      scope: 'constant.language',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: [
        'constant.numeric',
        'entity.name.operator.custom-literal.number',
        'keyword.operator.plus.exponent',
        'keyword.operator.minus.exponent',
      ],
      settings: {
        foreground: '#b5cea8',
      },
    },
    {
      scope: 'constant.regexp',
      settings: {
        foreground: '#646695',
      },
    },
    {
      scope: 'entity.name.tag',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'entity.name.tag.css',
      settings: {
        foreground: '#d7ba7d',
      },
    },
    {
      scope: 'entity.other.attribute-name',
      settings: {
        foreground: '#9cdcfe',
      },
    },
    {
      scope: [
        'entity.other.attribute-name.class.css',
        'entity.other.attribute-name.class.mixin.css',
        'entity.other.attribute-name.id.css',
        'entity.other.attribute-name.parent-selector.css',
        'entity.other.attribute-name.pseudo-class.css',
        'entity.other.attribute-name.pseudo-element.css',
        'source.css.less entity.other.attribute-name.id',
        'entity.other.attribute-name.attribute.scss',
        'entity.other.attribute-name.scss',
      ],
      settings: {
        foreground: '#d7ba7d',
      },
    },
    {
      scope: 'invalid',
      settings: {
        foreground: '#f44747',
      },
    },
    {
      scope: 'markup.underline',
      settings: {
        fontStyle: 'underline',
      },
    },
    {
      scope: 'markup.bold',
      settings: {
        fontStyle: 'bold',
        foreground: '#569cd6',
      },
    },
    {
      scope: 'markup.heading',
      settings: {
        fontStyle: 'bold',
        foreground: '#569cd6',
      },
    },
    {
      scope: 'markup.italic',
      settings: {
        fontStyle: 'italic',
      },
    },
    {
      scope: 'markup.inserted',
      settings: {
        foreground: '#b5cea8',
      },
    },
    {
      scope: 'markup.deleted',
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      scope: 'markup.changed',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'punctuation.definition.quote.begin.markdown',
      settings: {
        foreground: '#6A9955',
      },
    },
    {
      scope: 'punctuation.definition.list.begin.markdown',
      settings: {
        foreground: '#6796e6',
      },
    },
    {
      scope: 'markup.inline.raw',
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      name: 'brackets of XML/HTML tags',
      scope: 'punctuation.definition.tag',
      settings: {
        foreground: '#808080',
      },
    },
    {
      scope: ['meta.preprocessor', 'entity.name.function.preprocessor'],
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'meta.preprocessor.string',
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      scope: 'meta.preprocessor.numeric',
      settings: {
        foreground: '#b5cea8',
      },
    },
    {
      scope: 'meta.structure.dictionary.key.python',
      settings: {
        foreground: '#9cdcfe',
      },
    },
    {
      scope: 'meta.diff.header',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'storage',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'storage.type',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: ['storage.modifier', 'keyword.operator.noexcept'],
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: ['string', 'entity.name.operator.custom-literal.string', 'meta.embedded.assembly'],
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      scope: 'string.tag',
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      scope: 'string.value',
      settings: {
        foreground: '#ce9178',
      },
    },
    {
      scope: 'string.regexp',
      settings: {
        foreground: '#d16969',
      },
    },
    {
      name: 'String interpolation',
      scope: [
        'punctuation.definition.template-expression.begin',
        'punctuation.definition.template-expression.end',
        'punctuation.section.embedded',
      ],
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      name: 'Reset JavaScript string interpolation expression',
      scope: 'meta.template.expression',
      settings: {
        foreground: '#d4d4d4',
      },
    },
    {
      scope: [
        'support.type.vendored.property-name',
        'support.type.property-name',
        'variable.css',
        'variable.scss',
        'variable.other.less',
        'source.coffee.embedded',
      ],
      settings: {
        foreground: '#9cdcfe',
      },
    },
    {
      scope: 'keyword',
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'keyword.operator',
      settings: {
        foreground: '#d4d4d4',
      },
    },
    {
      scope: [
        'keyword.operator.new',
        'keyword.operator.expression',
        'keyword.operator.cast',
        'keyword.operator.sizeof',
        'keyword.operator.alignof',
        'keyword.operator.typeid',
        'keyword.operator.alignas',
        'keyword.operator.instanceof',
        'keyword.operator.logical.python',
        'keyword.operator.wordlike',
      ],
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'keyword.other.unit',
      settings: {
        foreground: '#b5cea8',
      },
    },
    {
      scope: ['punctuation.section.embedded.begin.php', 'punctuation.section.embedded.end.php'],
      settings: {
        foreground: '#569cd6',
      },
    },
    {
      scope: 'support.function.git-rebase',
      settings: {
        foreground: '#9cdcfe',
      },
    },
    {
      scope: 'constant.sha.git-rebase',
      settings: {
        foreground: '#b5cea8',
      },
    },
    {
      name: 'coloring of the Java import and package identifiers',
      scope: [
        'storage.modifier.import.java',
        'variable.language.wildcard.java',
        'storage.modifier.package.java',
      ],
      settings: {
        foreground: '#d4d4d4',
      },
    },
    {
      name: 'this.self',
      scope: 'variable.language',
      settings: {
        foreground: '#569cd6',
      },
    },
  ],
};

export default theme;
