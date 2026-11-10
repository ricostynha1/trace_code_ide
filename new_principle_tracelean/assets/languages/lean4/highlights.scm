; Lean 4 highlights.scm, for the command-level grammar in vendor/tree-sitter-lean4

; Keywords: the commands, their modifiers, and the words a body is written with
[
  "def" "theorem" "lemma" "abbrev" "instance" "example" "structure" "class"
  "inductive" "axiom" "opaque" "namespace" "section" "end" "mutual"
  "noncomputable" "partial" "protected" "private" "public" "meta" "unsafe"
  "scoped" "local" "nonrec"
  "open" "export" "variable" "universe" "set_option" "attribute" "import"
  "include" "omit" "macro" "macro_rules" "syntax" "notation" "infix" "infixl"
  "infixr" "prefix" "postfix" "elab" "elab_rules" "initialize"
  "builtin_initialize" "declare_syntax_cat" "add_decl_doc" "deriving" "prelude"
  "fun" "λ" "match" "with" "let" "have" "show" "from" "if" "then" "else" "do"
  "by" "return" "for" "in" "at" "where" "extends" "termination_by"
  "decreasing_by" "calc" "unless" "mut" "try" "catch" "finally" "sorry"
  "nomatch" "nofun" "suffices" "obtain" "using"
] @keyword

(hash_command) @keyword

; What a command declares
(definition name: (identifier) @function)
(axiom name: (identifier) @function)
(opaque name: (identifier) @function)
(structure name: (identifier) @type)
(inductive name: (identifier) @type)
(class_inductive name: (identifier) @type)
(namespace name: (identifier) @type)
(section name: (identifier) @type)
(end name: (identifier) @type)

; Built-in types
((identifier) @type
 (#match? @type "^(Type|Prop|Sort|Nat|Int|Bool|String|Char|List|Array|Option|Unit|True|False)$"))

(number) @number
(string) @string
(char) @string
(name_literal) @constant
(comment) @comment
(operator) @operator

["(" ")" "[" "]" "{" "}" "⟨" "⟩" "⦃" "⦄" "@["] @punctuation.bracket
[";" ","] @punctuation.delimiter
