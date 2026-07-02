; Markdown highlights_inline.scm — INLINE grammar only

; Emphasis
(emphasis) @markup.italic
(strong_emphasis) @markup.bold
(emphasis_delimiter) @punctuation.delimiter

; Code
(code_span) @string

; Links
(link_text) @markup.link
(link_destination) @string
(link_label) @markup.link
(image_description) @markup.link
(uri_autolink) @markup.link

; HTML inline
(html_tag) @keyword
