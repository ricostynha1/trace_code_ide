; Lean 4 highlights.scm
; Original — based on tree-sitter-lean4 node kinds

; Keywords
[
  "def"
  "theorem"
  "lemma"
  "example"
  "structure"
  "class"
  "instance"
  "inductive"
  "namespace"
  "section"
  "open"
  "variable"
  "axiom"
  "noncomputable"
  "private"
  "protected"
  "partial"
  "unsafe"
  "where"
  "with"
  "match"
  "do"
  "let"
  "have"
  "show"
  "if"
  "then"
  "else"
  "for"
  "in"
  "return"
  "import"
  "universe"
  "set_option"
  "attribute"
  "deriving"
  "extends"
  "abbrev"
  "opaque"
  "end"
  "macro"
  "syntax"
  "elab"
  "notation"
  "by"
  "fun"
] @keyword

; sorry/admit/prelude/mutual not anonymous nodes — match as identifiers
((identifier) @keyword
 (#match? @keyword "^(sorry|admit|prelude|mutual)$"))

; Built-in types
((identifier) @type
 (#match? @type "^(Type|Prop|Sort|Nat|Int|Bool|String|True|False)$"))

; Numbers
(number) @number

; Strings
(string) @string
(char) @string

; Comments
(comment) @comment

; Identifiers
(identifier) @variable

; Operators
[
  ":="
  "=>"
  "->"
  "<-"
  "="
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  ":"
  "|"
] @operator

; Punctuation
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[";" "," "."] @punctuation.delimiter
