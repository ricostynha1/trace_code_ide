; TypeScript highlights.scm
; Written for TraceLean against tree-sitter-typescript 0.23, in the capture
; names the Rust and Lean queries use.

[
  "as"
  "async"
  "await"
  "break"
  "case"
  "class"
  "const"
  "continue"
  "default"
  "else"
  "enum"
  "export"
  "extends"
  "for"
  "from"
  "function"
  "if"
  "implements"
  "import"
  "in"
  "instanceof"
  "interface"
  "let"
  "new"
  "of"
  "return"
  "switch"
  "throw"
  "try"
  "catch"
  "type"
  "typeof"
  "var"
  "while"
] @keyword

[(true) (false) (null) (undefined)] @constant.builtin

(comment) @comment

[(string) (template_string)] @string
(escape_sequence) @string.escape

(number) @number

[(type_identifier) (predefined_type)] @type

(function_declaration name: (identifier) @function)
(method_definition name: (property_identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (member_expression property: (property_identifier) @function))

(property_identifier) @property
