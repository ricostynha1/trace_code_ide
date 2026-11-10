/**
 * @file Lean 4, read at the level of its commands.
 * @license MIT
 *
 * Lean's terms, tactics and notation are extensible by the files that use
 * them, so no fixed grammar of terms reads all of Lean. This grammar reads
 * what is fixed: the commands of a file, where each begins and ends, what
 * each declares and under which name. A command's body is the run of tokens
 * up to the next command; the external scanner decides where that is, the way
 * Lean's own layout does (a new line whose first word starts a command).
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const IDENT_START = '[\\p{L}_\\u2100-\\u214F]';
const IDENT_REST = '[\\p{L}\\p{N}_\'!?\\u2080-\\u209C\\u1D62-\\u1D6A\\u2100-\\u214F]';
const ATOM = `(${IDENT_START}${IDENT_REST}*|«[^»]*»)`;

// Words that may appear inside a body. Listed so they are highlighted as
// keywords; a word not listed reads as an identifier there.
const BODY_WORDS = [
  'fun', 'λ', 'match', 'with', 'let', 'have', 'show', 'from', 'if', 'then', 'else',
  'do', 'by', 'return', 'for', 'in', 'at', 'where', 'deriving', 'extends',
  'termination_by', 'decreasing_by', 'calc', 'unless', 'mut', 'try', 'catch',
  'finally', 'open', 'set_option', 'sorry', 'nomatch', 'nofun', 'suffices',
  'obtain', 'using',
];

const COMMAND_HEADS = [
  'open', 'export', 'variable', 'universe', 'set_option', 'attribute', 'import',
  'include', 'omit', 'macro', 'macro_rules', 'syntax', 'notation', 'infix',
  'infixl', 'infixr', 'prefix', 'postfix', 'elab', 'elab_rules',
  'initialize', 'builtin_initialize', 'declare_syntax_cat', 'add_decl_doc',
  'deriving', 'prelude',
];

module.exports = grammar({
  name: 'lean',

  extras: $ => [$.comment, /\s/],

  // `comment` is also a rule below: the scanner reads nested comments, and
  // leaves one case to the rule (a doc comment before a constructor).
  externals: $ => [$._command_end, $.comment, $._error_sentinel],

  word: $ => $.identifier,

  conflicts: $ => [],

  rules: {
    source_file: $ => repeat($._command),

    _command: $ => seq(
      optional($.attributes),
      repeat($._modifier),
      choice(
        $.definition,
        $.structure,
        $.inductive,
        $.class_inductive,
        $.axiom,
        $.opaque,
        $.example,
        $.namespace,
        $.section,
        $.end,
        $.mutual,
        $.command,
      ),
    ),

    _modifier: _ => choice(
      'noncomputable', 'partial', 'protected', 'private', 'public', 'meta',
      'unsafe', 'scoped', 'local', 'nonrec',
    ),

    attributes: $ => seq('@[', repeat($._attribute_token), ']'),

    _attribute_token: $ => choice($._atom, seq('[', repeat($._attribute_token), ']')),

    // def, theorem, lemma, abbrev and instance: `instance` alone may be anonymous.
    definition: $ => choice(
      seq(
        field('kind', choice('def', 'theorem', 'lemma', 'abbrev')),
        field('name', $._name),
        optional($.body),
        $._command_end,
      ),
      seq(
        field('kind', 'instance'),
        optional(field('name', $._name)),
        optional($.body),
        $._command_end,
      ),
    ),

    structure: $ => seq(choice('structure', 'class'), field('name', $._name), optional($.body), $._command_end),
    inductive: $ => seq('inductive', field('name', $._name), optional($.body), $._command_end),
    class_inductive: $ => seq('class', 'inductive', field('name', $._name), optional($.body), $._command_end),
    axiom: $ => seq('axiom', field('name', $._name), optional($.body), $._command_end),
    opaque: $ => seq('opaque', field('name', $._name), optional($.body), $._command_end),
    example: $ => seq('example', optional($.body), $._command_end),

    namespace: $ => seq('namespace', field('name', $._name), $._command_end),
    section: $ => seq('section', optional(field('name', $._name)), $._command_end),
    end: $ => seq('end', optional(field('name', $._name)), $._command_end),
    mutual: $ => seq('mutual', $._command_end),

    // Every other command: a known head, `#eval`-like, or a command a file
    // defined for itself (`run_cmd`, `register_option`, …).
    command: $ => seq(
      field('head', choice(...COMMAND_HEADS, $.hash_command, $.identifier)),
      optional($.body),
      $._command_end,
    ),

    hash_command: _ => token(new RegExp(`#${IDENT_START}${IDENT_REST}*`, 'u')),

    body: $ => repeat1($._token),

    _token: $ => choice($._atom, '[', ']', '@[', ...BODY_WORDS),

    _atom: $ => choice(
      $.identifier,
      $.number,
      $.string,
      $.char,
      $.name_literal,
      $.operator,
      $.hash_command,
      $.quotation,
      '(', ')', '{', '}', '⟨', '⟩', '⦃', '⦄', ',', ';',
    ),

    // `` `(…) `` and `` `(category| …) ``: syntax written as data. Its
    // parentheses are matched, so a command quoted inside never ends the body.
    quotation: $ => seq(/`\(([A-Za-z_][A-Za-z0-9_.]*\|)?/, repeat($._quoted), ')'),

    _quoted: $ => choice(
      $._atom_in_quotation,
      seq('(', repeat($._quoted), ')'),
      '[', ']', '@[', ...BODY_WORDS,
    ),

    _atom_in_quotation: $ => choice(
      $.identifier, $.number, $.string, $.char, $.name_literal, $.operator,
      $.hash_command, $.quotation, '{', '}', '⟨', '⟩', '⦃', '⦄', ',', ';',
    ),

    _name: $ => prec(1, $.identifier),

    identifier: _ => token(new RegExp(`${ATOM}(\\.${ATOM})*`, 'u')),

    number: _ => token(choice(
      /0x[0-9a-fA-F_]+/,
      /0b[01_]+/,
      /0o[0-7_]+/,
      /[0-9][0-9_]*(\.[0-9]+)?([eE][+-]?[0-9]+)?/,
    )),

    string: _ => token(choice(
      /"([^"\\]|\\(.|\n))*"/,
      /r"[^"]*"/,
      /r#"([^"]|"[^#])*"#/,
    )),

    char: _ => token(/'(\\(x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^'\\\n])'/),

    name_literal: _ => token(/`+[^\s()\[\]{}⟨⟩,"`]*/),

    operator: _ => token(prec(-1, choice(
      /[^\s\p{L}0-9_"'`()\[\]{}⟨⟩⦃⦄«»,;#@]+/u,
      "'", '#', '@',
    ))),

    comment: _ => token(choice(
      /--[^\n]*/,
      /\/-([^-]|-+[^\/-])*-+\//,
    )),
  },
});
