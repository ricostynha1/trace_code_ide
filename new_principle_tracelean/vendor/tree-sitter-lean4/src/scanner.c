/*
 * Where a Lean command ends.
 *
 * A command's body is a run of tokens; it ends where the next command begins.
 * Lean marks that by layout: a command starts on a line of its own. So the end
 * is decided at a line break, by the first word of the next line:
 *
 *   at column 0, any word except those that continue a body there (`where`,
 *   `termination_by`, `deriving` without `instance`, …), and `@[`, `#…` or a
 *   doc comment;
 *   indented (as inside `mutual` or an indented `section`), a declaration
 *   keyword after any attributes and modifiers, a scoping command, or `open`,
 *   `set_option` or `attribute` unless the line ends in `in` (which a proof
 *   writes as `open Classical in`).
 *
 * The end is a zero-width token at the start of the next command. A
 * declaration's extent is read from its last token, not from this end, so the
 * blank lines and comments before the next command are not part of it.
 *
 * Comments are read here too, because Lean's block comments nest. A doc
 * comment at column 0 followed by `|` documents a constructor; that one is
 * left to the grammar's own rule for comments.
 */

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <string.h>

enum TokenType { COMMAND_END, COMMENT, ERROR_SENTINEL };

void *tree_sitter_lean_external_scanner_create(void) { return NULL; }
void tree_sitter_lean_external_scanner_destroy(void *payload) { (void)payload; }
unsigned tree_sitter_lean_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}
void tree_sitter_lean_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static bool is_word_char(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '\'' ||
         c == '!' || c == '?' || c == '.' || c >= 0x80;
}

static bool is_word_start(int32_t c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }

#define WORD_MAX 32

/* Reads a word; a word longer than WORD_MAX, or with a non-ASCII letter, is
   returned as "" so it matches no keyword. */
static void read_word(TSLexer *lexer, char *word) {
  unsigned n = 0;
  bool plain = true;
  while (!lexer->eof(lexer) && is_word_char(lexer->lookahead)) {
    if (lexer->lookahead >= 0x80 || n + 1 >= WORD_MAX) {
      plain = false;
    } else {
      word[n++] = (char)lexer->lookahead;
    }
    advance(lexer);
  }
  word[plain ? n : 0] = '\0';
}

static bool one_of(const char *word, const char *const *words) {
  for (unsigned i = 0; words[i]; i++) {
    if (strcmp(word, words[i]) == 0) return true;
  }
  return false;
}

static const char *const DECLARATIONS[] = {
  "def", "theorem", "lemma", "abbrev", "instance", "example", "structure", "class",
  "inductive", "axiom", "opaque", NULL,
};

static const char *const MODIFIERS[] = {
  "noncomputable", "partial", "protected", "private", "public", "meta", "unsafe", "scoped", "local", "nonrec", NULL,
};

/* Commands that also start a command when indented. */
static const char *const SCOPING[] = {
  "variable", "universe", "section", "namespace", "end", "mutual", "export", "include", "omit", NULL,
};

/* Commands that, indented, may instead be a body's `… in`. */
static const char *const PREFIXING[] = { "open", "set_option", "attribute", NULL };

/* Words that continue a body even at column 0. */
static const char *const CONTINUING[] = {
  "where", "termination_by", "decreasing_by", "with", "then", "else", "do", "by", "fun", "at", "from",
  "in", "using", "show", "match", "let", "have", "if", "calc", NULL,
};

static void skip_spaces(TSLexer *lexer) {
  while (!lexer->eof(lexer) && (lexer->lookahead == ' ' || lexer->lookahead == '\t')) advance(lexer);
}

/* Skips `[ … ]` with nesting; the lexer is on `[`. */
static void skip_brackets(TSLexer *lexer) {
  unsigned depth = 0;
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '[') depth++;
    if (lexer->lookahead == ']' && --depth == 0) {
      advance(lexer);
      return;
    }
    if (lexer->lookahead == '\n') return;
    advance(lexer);
  }
}

/* Skips a block comment, nested; the lexer is just past its `/-`. False when
   the file ends before the comment does. */
static bool skip_block_comment(TSLexer *lexer) {
  unsigned depth = 1;
  while (!lexer->eof(lexer) && depth > 0) {
    int32_t c = lexer->lookahead;
    advance(lexer);
    if (c == '/' && lexer->lookahead == '-') {
      advance(lexer);
      depth++;
    } else if (c == '-' && lexer->lookahead == '/') {
      advance(lexer);
      depth--;
    }
  }
  return depth == 0;
}

/* Does the rest of the line, before any `--` comment, end in the word `in`? */
static bool ends_in_in(TSLexer *lexer) {
  bool in = false;
  while (!lexer->eof(lexer) && lexer->lookahead != '\n') {
    int32_t c = lexer->lookahead;
    if (c == ' ' || c == '\t' || c == '\r') {
      advance(lexer);
    } else if (is_word_start(c)) {
      char word[WORD_MAX];
      read_word(lexer, word);
      in = strcmp(word, "in") == 0;
    } else {
      advance(lexer);
      if (c == '-' && lexer->lookahead == '-') break;
      in = false;
    }
  }
  return in;
}

/* The first word of an indented line, after attributes and modifiers: does
   it start a command? The lexer is on the line's first character. */
static bool declares(TSLexer *lexer, char *word) {
  for (;;) {
    if (lexer->lookahead == '@') {
      advance(lexer);
      if (lexer->lookahead != '[') return false;
      skip_brackets(lexer);
      skip_spaces(lexer);
      continue;
    }
    if (!is_word_start(lexer->lookahead)) return false;
    read_word(lexer, word);
    if (one_of(word, MODIFIERS)) {
      skip_spaces(lexer);
      continue;
    }
    if (one_of(word, DECLARATIONS) || one_of(word, SCOPING)) return true;
    return one_of(word, PREFIXING) && !ends_in_in(lexer);
  }
}

static bool skip_whitespace(TSLexer *lexer) {
  bool new_line = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '\n') {
      new_line = true;
    } else if (c != ' ' && c != '\t' && c != '\r' && c != '\f') {
      break;
    }
    skip(lexer);
  }
  return new_line;
}

bool tree_sitter_lean_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;
  if (valid_symbols[ERROR_SENTINEL]) return false;
  bool ending = valid_symbols[COMMAND_END];

  bool new_line = skip_whitespace(lexer);
  if (lexer->eof(lexer)) {
    lexer->result_symbol = COMMAND_END;
    return ending;
  }
  bool first = new_line && lexer->get_column(lexer) == 0;
  int32_t c = lexer->lookahead;

  if (c == '-') {
    advance(lexer);
    if (lexer->lookahead != '-' || !valid_symbols[COMMENT]) return false;
    while (!lexer->eof(lexer) && lexer->lookahead != '\n') advance(lexer);
    lexer->mark_end(lexer);
    lexer->result_symbol = COMMENT;
    return true;
  }

  if (c == '/') {
    lexer->mark_end(lexer);
    advance(lexer);
    if (lexer->lookahead != '-') return false;
    advance(lexer);
    bool doc = lexer->lookahead == '-' || lexer->lookahead == '!';
    if (!skip_block_comment(lexer)) return false;
    if (ending && first && doc) {
      skip_whitespace(lexer);
      if (lexer->lookahead == '|') return false;
      lexer->result_symbol = COMMAND_END;
      return true;
    }
    if (!valid_symbols[COMMENT]) return false;
    lexer->mark_end(lexer);
    lexer->result_symbol = COMMENT;
    return true;
  }

  if (!ending || !new_line) return false;
  lexer->result_symbol = COMMAND_END;
  lexer->mark_end(lexer);

  char word[WORD_MAX];
  if (!first) return declares(lexer, word);
  if (c == '#') return true;
  if (c == '@') {
    advance(lexer);
    return lexer->lookahead == '[';
  }
  if (!is_word_start(c)) return false;
  read_word(lexer, word);
  if (strcmp(word, "deriving") == 0) {
    skip_spaces(lexer);
    read_word(lexer, word);
    return strcmp(word, "instance") == 0;
  }
  return word[0] != '\0' && !one_of(word, CONTINUING);
}
