; Rust highlights.scm
; Adapted from tree-sitter-rust community queries (MIT)

; Keywords
[
  "let"
  "fn"
  "pub"
  "struct"
  "enum"
  "impl"
  "trait"
  "use"
  "mod"
  "where"
  "as"
  "in"
  "for"
  "while"
  "loop"
  "if"
  "else"
  "match"
  "return"
  "break"
  "continue"
  "async"
  "await"
  "move"
  "ref"
  "type"
  "const"
  "static"
  "unsafe"
  "extern"
  "dyn"
] @keyword

; mut, crate, self, super are named nodes in this grammar version
(mutable_specifier) @keyword
(crate) @keyword
(self) @keyword
(super) @keyword

; Built-in constants
((identifier) @constant.builtin
 (#match? @constant.builtin "^(true|false)$"))

; Functions
(function_item
  name: (identifier) @function)

(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_expression
    field: (field_identifier) @function.call))

; Macros
(macro_invocation
  macro: (identifier) @function.macro)

(macro_definition
  name: (identifier) @function.macro)

; Types
(type_identifier) @type
(primitive_type) @type.builtin

; Properties / fields
(field_identifier) @property

; Strings
(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(escape_sequence) @string.escape

; Numbers
(integer_literal) @number
(float_literal) @number

; Comments
(line_comment) @comment
(block_comment) @comment

; Attributes
(attribute_item) @keyword

; Identifiers
(identifier) @variable

; Operators
[
  "!"
  "!="
  "%"
  "&"
  "&&"
  "*"
  "+"
  "-"
  "/"
  "<"
  "<="
  "="
  "=="
  ">"
  ">="
  "|"
  "||"
  "^"
  "+="
  "-="
  "*="
  "/="
  "=>"
  "->"
  ".."
  "..="
] @operator

; Punctuation
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[";" "," "." "::" ":"] @punctuation.delimiter
