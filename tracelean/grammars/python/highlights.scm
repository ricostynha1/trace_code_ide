; Python highlights.scm
; Adapted from tree-sitter-python community queries (MIT)

; Keywords
[
  "def"
  "class"
  "return"
  "if"
  "elif"
  "else"
  "for"
  "while"
  "import"
  "from"
  "as"
  "with"
  "try"
  "except"
  "finally"
  "raise"
  "pass"
  "break"
  "continue"
  "and"
  "or"
  "not"
  "in"
  "is"
  "lambda"
  "yield"
  "global"
  "nonlocal"
  "assert"
  "del"
  "async"
  "await"
] @keyword

; Built-in constants
((identifier) @constant.builtin
 (#match? @constant.builtin "^(True|False|None)$"))

; Functions
(function_definition
  name: (identifier) @function)

(call
  function: (identifier) @function.call)

(call
  function: (attribute
    attribute: (identifier) @function.call))

; Decorators
(decorator) @keyword

; Types (class definitions)
(class_definition
  name: (identifier) @type)

; Strings
(string) @string
(string_start) @string
(string_content) @string
(string_end) @string
(escape_sequence) @string.escape

; Numbers
(integer) @number
(float) @number

; Comments
(comment) @comment

; Identifiers
(identifier) @variable

; Parameters
(parameters
  (identifier) @variable.parameter)

; Properties / attributes
(attribute
  attribute: (identifier) @property)

; Operators
[
  "+"
  "-"
  "*"
  "/"
  "%"
  "//"
  "**"
  "<<"
  ">>"
  "&"
  "|"
  "^"
  "~"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "="
  "+="
  "-="
  "*="
  "/="
] @operator

; Punctuation
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[";" "," "." ":"] @punctuation.delimiter
