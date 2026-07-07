; C++ highlights.scm
; Adapted from tree-sitter-cpp community queries (MIT)

; Keywords
[
  "if"
  "else"
  "for"
  "while"
  "do"
  "switch"
  "case"
  "break"
  "continue"
  "return"
  "goto"
  "typedef"
  "struct"
  "union"
  "enum"
  "class"
  "public"
  "private"
  "protected"
  "virtual"
  "override"
  "const"
  "static"
  "extern"
  "inline"
  "volatile"
  "register"
  "template"
  "typename"
  "namespace"
  "using"
  "new"
  "delete"
  "throw"
  "try"
  "catch"
  "sizeof"
] @keyword

; 'auto' is a named node in this grammar version
(auto) @keyword

; Built-in constants
((identifier) @constant.builtin
 (#match? @constant.builtin "^(nullptr|true|false|NULL)$"))

; Functions
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @function))

(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_expression
    field: (field_identifier) @function.call))

; Types
(type_identifier) @type
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin

; Properties / fields
(field_identifier) @property

; Strings
(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(system_lib_string) @string
(escape_sequence) @string.escape

; Numbers
(number_literal) @number

; Comments
(comment) @comment

; Preprocessor
(preproc_include) @keyword
(preproc_def) @keyword
(preproc_ifdef) @keyword
(preproc_if) @keyword
(preproc_else) @keyword
"#include" @keyword
"#define" @keyword

; Identifiers
(identifier) @variable

; Operators
[
  "!"
  "!="
  "%"
  "&&"
  "&"
  "*"
  "+"
  "++"
  "-"
  "--"
  "/"
  "<"
  "<="
  "<<"
  "=="
  "="
  ">"
  ">="
  ">>"
  "^"
  "||"
  "|"
  "~"
  "->"
] @operator

; Punctuation
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[";" "," "." "::"] @punctuation.delimiter
