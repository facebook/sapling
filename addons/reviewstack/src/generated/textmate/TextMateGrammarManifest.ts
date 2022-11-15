export type TextMateGrammar = {
  type: 'json' | 'plist';
  /**
   * Grammar data as a string because parseRawGrammar() in vscode-textmate
   * takes the contents as a string, even if the type is json.
   */
  grammar: string;
};

type Grammar = {
  language?: string;
  injections: Array<string>;
  embeddedLanguages?: {[scopeName: string]: string};
  getGrammar: () => Promise<TextMateGrammar>;
};

const grammars: {[scopeName: string]: Grammar} = {
  'documentation.injection.js.jsx': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('documentation_injection_js_jsx_TextMateGrammar', 'json');
    },
  },
  'documentation.injection.ts': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('documentation_injection_ts_TextMateGrammar', 'json');
    },
  },
  'markdown.cabal.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: {'meta.embedded.block.cabal': 'cabal'},
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_cabal_codeblock_TextMateGrammar', 'json');
    },
  },
  'markdown.hack.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: {'meta.embedded.block.hack': 'hack'},
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_hack_codeblock_TextMateGrammar', 'json');
    },
  },
  'markdown.haskell.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: {'meta.embedded.block.haskell': 'haskell'},
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_haskell_codeblock_TextMateGrammar', 'json');
    },
  },
  'markdown.kotlin.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: {'meta.embedded.block.kotlin': 'kotlin'},
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_kotlin_codeblock_TextMateGrammar', 'json');
    },
  },
  'markdown.lhaskell.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: {'meta.embedded.block.lhaskell': 'lhaskell'},
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_lhaskell_codeblock_TextMateGrammar', 'json');
    },
  },
  'markdown.toml.frontmatter.codeblock': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('markdown_toml_frontmatter_codeblock_TextMateGrammar', 'plist');
    },
  },
  'source.asp.vb.net': {
    language: 'vb',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_asp_vb_net_TextMateGrammar', 'json');
    },
  },
  'source.batchfile': {
    language: 'bat',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_batchfile_TextMateGrammar', 'json');
    },
  },
  'source.c': {
    language: 'c',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_c_TextMateGrammar', 'json');
    },
  },
  'source.c.platform': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_c_platform_TextMateGrammar', 'json');
    },
  },
  'source.c2hs': {
    language: 'C2Hs',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_c2hs_TextMateGrammar', 'json');
    },
  },
  'source.cabal': {
    language: 'cabal',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_cabal_TextMateGrammar', 'json');
    },
  },
  'source.clojure': {
    language: 'clojure',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_clojure_TextMateGrammar', 'json');
    },
  },
  'source.coffee': {
    language: 'coffeescript',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_coffee_TextMateGrammar', 'json');
    },
  },
  'source.cpp': {
    language: 'cpp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_cpp_TextMateGrammar', 'json');
    },
  },
  'source.cpp.embedded.macro': {
    language: 'cpp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_cpp_embedded_macro_TextMateGrammar', 'json');
    },
  },
  'source.cs': {
    language: 'csharp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_cs_TextMateGrammar', 'json');
    },
  },
  'source.css': {
    language: 'css',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_css_TextMateGrammar', 'json');
    },
  },
  'source.css.less': {
    language: 'less',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_css_less_TextMateGrammar', 'json');
    },
  },
  'source.css.scss': {
    language: 'scss',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_css_scss_TextMateGrammar', 'json');
    },
  },
  'source.cuda-cpp': {
    language: 'cuda-cpp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_cuda-cpp_TextMateGrammar', 'json');
    },
  },
  'source.dart': {
    language: 'dart',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_dart_TextMateGrammar', 'json');
    },
  },
  'source.dockerfile': {
    language: 'dockerfile',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_dockerfile_TextMateGrammar', 'json');
    },
  },
  'source.fsharp': {
    language: 'fsharp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_fsharp_TextMateGrammar', 'json');
    },
  },
  'source.gdscript': {
    language: 'gdscript',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_gdscript_TextMateGrammar', 'json');
    },
  },
  'source.go': {
    language: 'go',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_go_TextMateGrammar', 'json');
    },
  },
  'source.groovy': {
    language: 'groovy',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_groovy_TextMateGrammar', 'json');
    },
  },
  'source.hack': {
    language: 'hack',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_hack_TextMateGrammar', 'json');
    },
  },
  'source.haskell': {
    language: 'haskell',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_haskell_TextMateGrammar', 'json');
    },
  },
  'source.hlsl': {
    language: 'hlsl',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_hlsl_TextMateGrammar', 'json');
    },
  },
  'source.hsc': {
    language: 'Hsc2Hs',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_hsc_TextMateGrammar', 'json');
    },
  },
  'source.ignore': {
    language: 'ignore',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_ignore_TextMateGrammar', 'json');
    },
  },
  'source.ini': {
    language: 'ini',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_ini_TextMateGrammar', 'json');
    },
  },
  'source.java': {
    language: 'java',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_java_TextMateGrammar', 'json');
    },
  },
  'source.js': {
    language: 'javascript',
    injections: ['documentation.injection.js.jsx'],
    embeddedLanguages: {
      'meta.tag.js': 'jsx-tags',
      'meta.tag.without-attributes.js': 'jsx-tags',
      'meta.tag.attributes.js': 'javascript',
      'meta.embedded.expression.js': 'javascript',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_js_TextMateGrammar', 'json');
    },
  },
  'source.js.jsx': {
    language: 'javascriptreact',
    injections: ['documentation.injection.js.jsx'],
    embeddedLanguages: {
      'meta.tag.js': 'jsx-tags',
      'meta.tag.without-attributes.js': 'jsx-tags',
      'meta.tag.attributes.js.jsx': 'javascriptreact',
      'meta.embedded.expression.js': 'javascriptreact',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_js_jsx_TextMateGrammar', 'json');
    },
  },
  'source.js.regexp': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_js_regexp_TextMateGrammar', 'plist');
    },
  },
  'source.julia': {
    language: 'julia',
    injections: [],
    embeddedLanguages: {
      'meta.embedded.inline.cpp': 'cpp',
      'meta.embedded.inline.javascript': 'javascript',
      'meta.embedded.inline.python': 'python',
      'meta.embedded.inline.r': 'r',
      'meta.embedded.inline.sql': 'sql',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_julia_TextMateGrammar', 'json');
    },
  },
  'source.kotlin': {
    language: 'kotlin',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_kotlin_TextMateGrammar', 'plist');
    },
  },
  'source.lua': {
    language: 'lua',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_lua_TextMateGrammar', 'json');
    },
  },
  'source.makefile': {
    language: 'makefile',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_makefile_TextMateGrammar', 'json');
    },
  },
  'source.objc': {
    language: 'objective-c',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_objc_TextMateGrammar', 'json');
    },
  },
  'source.objcpp': {
    language: 'objective-cpp',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_objcpp_TextMateGrammar', 'json');
    },
  },
  'source.perl': {
    language: 'perl',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_perl_TextMateGrammar', 'json');
    },
  },
  'source.perl.6': {
    language: 'perl6',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_perl_6_TextMateGrammar', 'json');
    },
  },
  'source.powershell': {
    language: 'powershell',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_powershell_TextMateGrammar', 'json');
    },
  },
  'source.python': {
    language: 'python',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_python_TextMateGrammar', 'json');
    },
  },
  'source.r': {
    language: 'r',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_r_TextMateGrammar', 'json');
    },
  },
  'source.regexp.python': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_regexp_python_TextMateGrammar', 'json');
    },
  },
  'source.ruby': {
    language: 'ruby',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_ruby_TextMateGrammar', 'json');
    },
  },
  'source.rust': {
    language: 'rust',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_rust_TextMateGrammar', 'json');
    },
  },
  'source.sassdoc': {
    language: undefined,
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_sassdoc_TextMateGrammar', 'json');
    },
  },
  'source.shell': {
    language: 'shellscript',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_shell_TextMateGrammar', 'json');
    },
  },
  'source.sql': {
    language: 'sql',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_sql_TextMateGrammar', 'json');
    },
  },
  'source.swift': {
    language: 'swift',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_swift_TextMateGrammar', 'json');
    },
  },
  'source.thrift': {
    language: 'thrift',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_thrift_TextMateGrammar', 'plist');
    },
  },
  'source.toml': {
    language: 'toml',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_toml_TextMateGrammar', 'plist');
    },
  },
  'source.ts': {
    language: 'typescript',
    injections: ['documentation.injection.ts'],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_ts_TextMateGrammar', 'json');
    },
  },
  'source.tsx': {
    language: 'typescriptreact',
    injections: ['documentation.injection.ts'],
    embeddedLanguages: {
      'meta.tag.tsx': 'jsx-tags',
      'meta.tag.without-attributes.tsx': 'jsx-tags',
      'meta.tag.attributes.tsx': 'typescriptreact',
      'meta.embedded.expression.tsx': 'typescriptreact',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_tsx_TextMateGrammar', 'json');
    },
  },
  'source.yaml': {
    language: 'yaml',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('source_yaml_TextMateGrammar', 'json');
    },
  },
  'text.git-commit': {
    language: 'git-commit',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_git-commit_TextMateGrammar', 'json');
    },
  },
  'text.git-rebase': {
    language: 'git-rebase',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_git-rebase_TextMateGrammar', 'json');
    },
  },
  'text.html.basic': {
    language: undefined,
    injections: [],
    embeddedLanguages: {
      'text.html': 'html',
      'source.css': 'css',
      'source.js': 'javascript',
      'source.python': 'python',
      'source.smarty': 'smarty',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_html_basic_TextMateGrammar', 'json');
    },
  },
  'text.html.cshtml': {
    language: 'razor',
    injections: [],
    embeddedLanguages: {
      'section.embedded.source.cshtml': 'csharp',
      'source.css': 'css',
      'source.js': 'javascript',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_html_cshtml_TextMateGrammar', 'json');
    },
  },
  'text.html.derivative': {
    language: 'html',
    injections: [],
    embeddedLanguages: {
      'text.html': 'html',
      'source.css': 'css',
      'source.js': 'javascript',
      'source.python': 'python',
      'source.smarty': 'smarty',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_html_derivative_TextMateGrammar', 'json');
    },
  },
  'text.html.handlebars': {
    language: 'handlebars',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_html_handlebars_TextMateGrammar', 'json');
    },
  },
  'text.html.markdown': {
    language: 'markdown',
    injections: [
      'markdown.cabal.codeblock',
      'markdown.hack.codeblock',
      'markdown.haskell.codeblock',
      'markdown.kotlin.codeblock',
      'markdown.lhaskell.codeblock',
      'markdown.toml.frontmatter.codeblock',
    ],
    embeddedLanguages: {
      'meta.embedded.block.html': 'html',
      'source.js': 'javascript',
      'source.css': 'css',
      'meta.embedded.block.frontmatter': 'yaml',
      'meta.embedded.block.css': 'css',
      'meta.embedded.block.ini': 'ini',
      'meta.embedded.block.java': 'java',
      'meta.embedded.block.lua': 'lua',
      'meta.embedded.block.makefile': 'makefile',
      'meta.embedded.block.perl': 'perl',
      'meta.embedded.block.r': 'r',
      'meta.embedded.block.ruby': 'ruby',
      'meta.embedded.block.php': 'php',
      'meta.embedded.block.sql': 'sql',
      'meta.embedded.block.vs_net': 'vs_net',
      'meta.embedded.block.xml': 'xml',
      'meta.embedded.block.xsl': 'xsl',
      'meta.embedded.block.yaml': 'yaml',
      'meta.embedded.block.dosbatch': 'dosbatch',
      'meta.embedded.block.clojure': 'clojure',
      'meta.embedded.block.coffee': 'coffee',
      'meta.embedded.block.c': 'c',
      'meta.embedded.block.cpp': 'cpp',
      'meta.embedded.block.diff': 'diff',
      'meta.embedded.block.dockerfile': 'dockerfile',
      'meta.embedded.block.go': 'go',
      'meta.embedded.block.groovy': 'groovy',
      'meta.embedded.block.pug': 'jade',
      'meta.embedded.block.javascript': 'javascript',
      'meta.embedded.block.json': 'json',
      'meta.embedded.block.jsonc': 'jsonc',
      'meta.embedded.block.less': 'less',
      'meta.embedded.block.objc': 'objc',
      'meta.embedded.block.scss': 'scss',
      'meta.embedded.block.perl6': 'perl6',
      'meta.embedded.block.powershell': 'powershell',
      'meta.embedded.block.python': 'python',
      'meta.embedded.block.rust': 'rust',
      'meta.embedded.block.scala': 'scala',
      'meta.embedded.block.shellscript': 'shellscript',
      'meta.embedded.block.typescript': 'typescript',
      'meta.embedded.block.typescriptreact': 'typescriptreact',
      'meta.embedded.block.csharp': 'csharp',
      'meta.embedded.block.fsharp': 'fsharp',
    },
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_html_markdown_TextMateGrammar', 'json');
    },
  },
  'text.log': {
    language: 'log',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_log_TextMateGrammar', 'json');
    },
  },
  'text.pug': {
    language: 'jade',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_pug_TextMateGrammar', 'json');
    },
  },
  'text.tex.latex.haskell': {
    language: 'literate haskell',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_tex_latex_haskell_TextMateGrammar', 'json');
    },
  },
  'text.xml': {
    language: 'xml',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_xml_TextMateGrammar', 'json');
    },
  },
  'text.xml.xsl': {
    language: 'xsl',
    injections: [],
    embeddedLanguages: undefined,
    getGrammar(): Promise<TextMateGrammar> {
      return fetchGrammar('text_xml_xsl_TextMateGrammar', 'json');
    },
  },
};

export type LanguageConfiguration = {
  id: string;
  extensions?: string[];
  filenames?: string[];
  filenamePatterns?: string[];
  firstLine?: string;
  aliases?: string[];
  mimetypes?: string[];
};

async function fetchGrammar(moduleName: string, type: 'json' | 'plist'): Promise<TextMateGrammar> {
  const uri = `/generated/textmate/${moduleName}.${type}`;
  const response = await fetch(uri);
  const grammar = await response.text();
  return {type, grammar};
}

const languages: {[language: string]: LanguageConfiguration} = {
  C2Hs: {
    id: 'C2Hs',
    aliases: ['C2Hs', 'c2hs'],
    extensions: ['.chs'],
  },
  Hsc2Hs: {
    id: 'Hsc2Hs',
    aliases: ['Hsc2Hs', 'HsC2Hs', 'hsc2hs'],
    extensions: ['.hsc'],
  },
  bat: {
    id: 'bat',
    extensions: ['.bat', '.cmd'],
    aliases: ['Batch', 'bat'],
  },
  c: {
    id: 'c',
    extensions: ['.c', '.i'],
    aliases: ['C', 'c'],
  },
  cabal: {
    id: 'cabal',
    aliases: ['Cabal', 'cabal'],
    extensions: ['.cabal'],
  },
  clojure: {
    id: 'clojure',
    aliases: ['Clojure', 'clojure'],
    extensions: ['.clj', '.cljs', '.cljc', '.cljx', '.clojure', '.edn'],
  },
  coffeescript: {
    id: 'coffeescript',
    extensions: ['.coffee', '.cson', '.iced'],
    aliases: ['CoffeeScript', 'coffeescript', 'coffee'],
  },
  cpp: {
    id: 'cpp',
    extensions: [
      '.cpp',
      '.cc',
      '.cxx',
      '.c++',
      '.hpp',
      '.hh',
      '.hxx',
      '.h++',
      '.h',
      '.ii',
      '.ino',
      '.inl',
      '.ipp',
      '.ixx',
      '.tpp',
      '.txx',
      '.hpp.in',
      '.h.in',
      '.cu',
      '.cuh',
    ],
    aliases: ['C++', 'Cpp', 'cpp'],
  },
  csharp: {
    id: 'csharp',
    extensions: ['.cs', '.csx', '.cake'],
    aliases: ['C#', 'csharp'],
  },
  css: {
    id: 'css',
    aliases: ['CSS', 'css'],
    extensions: ['.css'],
    mimetypes: ['text/css'],
  },
  'cuda-cpp': {
    id: 'cuda-cpp',
    extensions: ['.cu', '.cuh'],
    aliases: ['CUDA C++'],
  },
  dart: {
    id: 'dart',
    extensions: ['.dart'],
    aliases: ['Dart'],
  },
  dockercompose: {
    id: 'dockercompose',
    aliases: ['Compose', 'compose'],
    filenamePatterns: [
      'compose.yml',
      'compose.yaml',
      'compose.*.yml',
      'compose.*.yaml',
      '*docker*compose*.yml',
      '*docker*compose*.yaml',
    ],
  },
  dockerfile: {
    id: 'dockerfile',
    extensions: ['.dockerfile', '.containerfile'],
    filenames: ['Dockerfile', 'Containerfile'],
    filenamePatterns: ['Dockerfile.*', 'Containerfile.*'],
    aliases: ['Docker', 'Dockerfile', 'Containerfile'],
  },
  fsharp: {
    id: 'fsharp',
    extensions: ['.fs', '.fsi', '.fsx', '.fsscript'],
    aliases: ['F#', 'FSharp', 'fsharp'],
  },
  gdscript: {
    id: 'gdscript',
    aliases: ['GDScript', 'gdscript'],
    extensions: ['.gd'],
  },
  'git-commit': {
    id: 'git-commit',
    aliases: ['Git Commit Message', 'git-commit'],
    filenames: ['COMMIT_EDITMSG', 'MERGE_MSG'],
  },
  'git-rebase': {
    id: 'git-rebase',
    aliases: ['Git Rebase Message', 'git-rebase'],
    filenames: ['git-rebase-todo'],
  },
  go: {
    id: 'go',
    extensions: ['.go'],
    aliases: ['Go'],
  },
  groovy: {
    id: 'groovy',
    aliases: ['Groovy', 'groovy'],
    extensions: ['.groovy', '.gvy', '.gradle', '.jenkinsfile', '.nf'],
    filenames: ['Jenkinsfile'],
    filenamePatterns: ['Jenkinsfile.*'],
    firstLine: '^#!.*\\bgroovy\\b',
  },
  hack: {
    id: 'hack',
    aliases: ['Hack', 'hacklang', 'php'],
    extensions: ['.php', '.hh', '.hack'],
    firstLine: '^<\\?hh\\b.*|#!.*hhvm.*$',
  },
  handlebars: {
    id: 'handlebars',
    extensions: ['.handlebars', '.hbs', '.hjs'],
    aliases: ['Handlebars', 'handlebars'],
    mimetypes: ['text/x-handlebars-template'],
  },
  haskell: {
    id: 'haskell',
    aliases: ['Haskell', 'haskell'],
    extensions: ['.hsig', 'hs-boot', '.hs'],
  },
  hlsl: {
    id: 'hlsl',
    extensions: ['.hlsl', '.hlsli', '.fx', '.fxh', '.vsh', '.psh', '.cginc', '.compute'],
    aliases: ['HLSL', 'hlsl'],
  },
  html: {
    id: 'html',
    extensions: [
      '.html',
      '.htm',
      '.shtml',
      '.xhtml',
      '.xht',
      '.mdoc',
      '.jsp',
      '.asp',
      '.aspx',
      '.jshtm',
      '.volt',
      '.ejs',
      '.rhtml',
    ],
    aliases: ['HTML', 'htm', 'html', 'xhtml'],
    mimetypes: [
      'text/html',
      'text/x-jshtm',
      'text/template',
      'text/ng-template',
      'application/xhtml+xml',
    ],
  },
  ignore: {
    id: 'ignore',
    aliases: ['Ignore', 'ignore'],
    extensions: ['.gitignore_global', '.gitignore'],
  },
  ini: {
    id: 'ini',
    extensions: ['.ini', '.bcfg', '.net'],
    aliases: ['Ini', 'ini', 'Hack Configuration', 'hack', 'hacklang'],
    filenames: ['.hhconfig', '.buckconfig', '.flowconfig'],
  },
  jade: {
    id: 'jade',
    extensions: ['.pug', '.jade'],
    aliases: ['Pug', 'Jade', 'jade'],
  },
  java: {
    id: 'java',
    extensions: ['.java', '.jav'],
    aliases: ['Java', 'java'],
  },
  javascript: {
    id: 'javascript',
    aliases: ['JavaScript', 'javascript', 'js'],
    extensions: ['.js', '.es6', '.mjs', '.cjs', '.pac'],
    filenames: ['jakefile'],
    firstLine: '^#!.*\\bnode',
    mimetypes: ['text/javascript'],
  },
  javascriptreact: {
    id: 'javascriptreact',
    aliases: ['JavaScript React', 'jsx'],
    extensions: ['.jsx'],
  },
  jsonc: {
    id: 'jsonc',
    filenames: ['tsconfig.json', 'jsconfig.json'],
    filenamePatterns: ['tsconfig.*.json', 'jsconfig.*.json', 'tsconfig-*.json', 'jsconfig-*.json'],
  },
  'jsx-tags': {
    id: 'jsx-tags',
    aliases: [],
  },
  julia: {
    id: 'julia',
    aliases: ['Julia', 'julia'],
    extensions: ['.jl'],
    firstLine: '^#!\\s*/.*\\bjulia[0-9.-]*\\b',
  },
  juliamarkdown: {
    id: 'juliamarkdown',
    aliases: ['Julia Markdown', 'juliamarkdown'],
    extensions: ['.jmd'],
  },
  kotlin: {
    id: 'kotlin',
    aliases: ['Kotlin', 'kotlin'],
    extensions: ['.kt'],
  },
  kotlinscript: {
    id: 'kotlinscript',
    aliases: ['Kotlinscript', 'kotlinscript'],
    extensions: ['.kts'],
  },
  less: {
    id: 'less',
    aliases: ['Less', 'less'],
    extensions: ['.less'],
    mimetypes: ['text/x-less', 'text/less'],
  },
  'literate haskell': {
    id: 'literate haskell',
    aliases: ['Literate Haskell', 'literate Haskell'],
    extensions: ['.lhs'],
  },
  log: {
    id: 'log',
    extensions: ['.log', '*.log.?'],
    aliases: ['Log'],
  },
  lua: {
    id: 'lua',
    extensions: ['.lua'],
    aliases: ['Lua', 'lua'],
  },
  makefile: {
    id: 'makefile',
    aliases: ['Makefile', 'makefile'],
    extensions: ['.mak', '.mk'],
    filenames: ['Makefile', 'makefile', 'GNUmakefile', 'OCamlMakefile'],
    firstLine: '^#!\\s*/usr/bin/make',
  },
  markdown: {
    id: 'markdown',
    aliases: ['Markdown', 'markdown'],
    extensions: [
      '.md',
      '.mkd',
      '.mdwn',
      '.mdown',
      '.markdown',
      '.markdn',
      '.mdtxt',
      '.mdtext',
      '.workbook',
    ],
  },
  'objective-c': {
    id: 'objective-c',
    extensions: ['.m'],
    aliases: ['Objective-C'],
  },
  'objective-cpp': {
    id: 'objective-cpp',
    extensions: ['.mm'],
    aliases: ['Objective-C++'],
  },
  perl: {
    id: 'perl',
    aliases: ['Perl', 'perl'],
    extensions: ['.pl', '.pm', '.pod', '.t', '.PL', '.psgi'],
    firstLine: '^#!.*\\bperl\\b',
  },
  perl6: {
    id: 'perl6',
    aliases: ['Perl 6', 'perl6'],
    extensions: ['.p6', '.pl6', '.pm6', '.nqp'],
    firstLine: '(^#!.*\\bperl6\\b)|use\\s+v6',
  },
  powershell: {
    id: 'powershell',
    extensions: ['.ps1', '.psm1', '.psd1', '.pssc', '.psrc'],
    aliases: ['PowerShell', 'powershell', 'ps', 'ps1'],
    firstLine: '^#!\\s*/.*\\bpwsh\\b',
  },
  properties: {
    id: 'properties',
    extensions: [
      '.properties',
      '.cfg',
      '.conf',
      '.directory',
      '.gitattributes',
      '.gitconfig',
      '.gitmodules',
      '.editorconfig',
      'cfg',
      'tres',
      'tscn',
      'godot',
      'gdns',
      'gdnlib',
      'import',
    ],
    filenames: ['gitconfig'],
    filenamePatterns: ['**/.config/git/config', '**/.git/config'],
    aliases: ['Properties', 'properties'],
  },
  python: {
    id: 'python',
    extensions: ['.py', '.rpy', '.pyw', '.cpy', '.gyp', '.gypi', '.pyi', '.ipy', '.pyt'],
    aliases: ['Python', 'py'],
    filenames: ['Snakefile', 'SConstruct', 'SConscript'],
    firstLine: '^#!\\s*/?.*\\bpython[0-9.-]*\\b',
  },
  r: {
    id: 'r',
    extensions: ['.r', '.rhistory', '.rprofile', '.rt'],
    aliases: ['R', 'r'],
  },
  razor: {
    id: 'razor',
    extensions: ['.cshtml', '.razor'],
    aliases: ['Razor', 'razor'],
    mimetypes: ['text/x-cshtml'],
  },
  ruby: {
    id: 'ruby',
    extensions: ['.rb', '.rbx', '.rjs', '.gemspec', '.rake', '.ru', '.erb', '.podspec', '.rbi'],
    filenames: [
      'rakefile',
      'gemfile',
      'guardfile',
      'podfile',
      'capfile',
      'cheffile',
      'hobofile',
      'vagrantfile',
      'appraisals',
      'rantfile',
      'berksfile',
      'berksfile.lock',
      'thorfile',
      'puppetfile',
      'dangerfile',
      'brewfile',
      'fastfile',
      'appfile',
      'deliverfile',
      'matchfile',
      'scanfile',
      'snapfile',
      'gymfile',
    ],
    aliases: ['Ruby', 'rb'],
    firstLine: '^#!\\s*/.*\\bruby\\b',
  },
  rust: {
    id: 'rust',
    extensions: ['.rs'],
    aliases: ['Rust', 'rust'],
  },
  scss: {
    id: 'scss',
    aliases: ['SCSS', 'scss'],
    extensions: ['.scss'],
    mimetypes: ['text/x-scss', 'text/scss'],
  },
  shellscript: {
    id: 'shellscript',
    aliases: ['Shell Script', 'shellscript', 'bash', 'sh', 'zsh', 'ksh', 'csh'],
    extensions: [
      '.sh',
      '.bash',
      '.bashrc',
      '.bash_aliases',
      '.bash_profile',
      '.bash_login',
      '.ebuild',
      '.profile',
      '.bash_logout',
      '.xprofile',
      '.xsession',
      '.xsessionrc',
      '.Xsession',
      '.zsh',
      '.zshrc',
      '.zprofile',
      '.zlogin',
      '.zlogout',
      '.zshenv',
      '.zsh-theme',
      '.ksh',
      '.csh',
      '.cshrc',
      '.tcshrc',
      '.yashrc',
      '.yash_profile',
    ],
    filenames: [
      'APKBUILD',
      'PKGBUILD',
      '.envrc',
      '.hushlogin',
      'zshrc',
      'zshenv',
      'zlogin',
      'zprofile',
      'zlogout',
      'bashrc_Apple_Terminal',
      'zshrc_Apple_Terminal',
    ],
    firstLine:
      '^#!.*\\b(bash|zsh|sh|ksh|dtksh|pdksh|mksh|ash|dash|yash|sh|csh|jcsh|tcsh|itcsh).*|^#\\s*-\\*-[^*]*mode:\\s*shell-script[^*]*-\\*-',
    mimetypes: ['text/x-shellscript'],
  },
  sql: {
    id: 'sql',
    extensions: ['.sql', '.dsql'],
    aliases: ['SQL'],
  },
  swift: {
    id: 'swift',
    aliases: ['Swift', 'swift'],
    extensions: ['.swift'],
  },
  thrift: {
    id: 'thrift',
    extensions: ['.thrift'],
  },
  toml: {
    id: 'toml',
    aliases: ['TOML', 'toml'],
    extensions: ['.toml', 'Pipfile'],
    mimetypes: ['text/x-toml'],
  },
  typescript: {
    id: 'typescript',
    aliases: ['TypeScript', 'ts', 'typescript'],
    extensions: ['.ts', '.cts', '.mts'],
  },
  typescriptreact: {
    id: 'typescriptreact',
    aliases: ['TypeScript React', 'tsx'],
    extensions: ['.tsx'],
  },
  vb: {
    id: 'vb',
    extensions: ['.vb', '.brs', '.vbs', '.bas', '.vba'],
    aliases: ['Visual Basic', 'vb'],
  },
  xml: {
    id: 'xml',
    extensions: [
      '.xml',
      '.xsd',
      '.ascx',
      '.atom',
      '.axml',
      '.axaml',
      '.bpmn',
      '.cpt',
      '.csl',
      '.csproj',
      '.csproj.user',
      '.dita',
      '.ditamap',
      '.dtd',
      '.ent',
      '.mod',
      '.dtml',
      '.fsproj',
      '.fxml',
      '.iml',
      '.isml',
      '.jmx',
      '.launch',
      '.menu',
      '.mxml',
      '.nuspec',
      '.opml',
      '.owl',
      '.proj',
      '.props',
      '.pt',
      '.publishsettings',
      '.pubxml',
      '.pubxml.user',
      '.rbxlx',
      '.rbxmx',
      '.rdf',
      '.rng',
      '.rss',
      '.shproj',
      '.storyboard',
      '.svg',
      '.targets',
      '.tld',
      '.tmx',
      '.vbproj',
      '.vbproj.user',
      '.vcxproj',
      '.vcxproj.filters',
      '.wsdl',
      '.wxi',
      '.wxl',
      '.wxs',
      '.xaml',
      '.xbl',
      '.xib',
      '.xlf',
      '.xliff',
      '.xpdl',
      '.xul',
      '.xoml',
    ],
    firstLine: '(\\<\\?xml.*)|(\\<svg)|(\\<\\!doctype\\s+svg)',
    aliases: ['XML', 'xml'],
  },
  xsl: {
    id: 'xsl',
    extensions: ['.xsl', '.xslt'],
    aliases: ['XSL', 'xsl'],
  },
  yaml: {
    id: 'yaml',
    aliases: ['YAML', 'yaml', 'YAML', 'yaml'],
    extensions: ['.yml', '.eyaml', '.eyml', '.yaml', '.cff'],
    firstLine: '^#cloud-config',
    filenames: ['stack.yaml.lock', '.prettierrc'],
  },
};

export {grammars, languages};
