; Markdown highlights.scm — BLOCK grammar only
; Inline-grammar queries live in highlights_inline.scm

; Headings — full node gets heading color
(atx_heading) @markup.heading

; Heading markers
(atx_heading
  (atx_h1_marker) @markup.heading.marker)
(atx_heading
  (atx_h2_marker) @markup.heading.marker)
(atx_heading
  (atx_h3_marker) @markup.heading.marker)
(atx_heading
  (atx_h4_marker) @markup.heading.marker)
(atx_heading
  (atx_h5_marker) @markup.heading.marker)
(atx_heading
  (atx_h6_marker) @markup.heading.marker)

(setext_heading) @markup.heading

; Code
(fenced_code_block) @string
(code_fence_content) @string
(info_string) @property

; Links
(link_destination) @string
(link_label) @markup.link

; Lists
[
  (list_marker_minus)
  (list_marker_plus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
] @punctuation.delimiter

; Block quotes
(block_quote_marker) @punctuation.delimiter

; Thematic break
(thematic_break) @punctuation.delimiter
