# Judging advice — REQ-LSP

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `encoding_round_trip` | agrees, with a note |

### `encoding_round_trip` — note

`toByteOffset` and `toCharacter` walk the same characters with the same widths
(utf8 bytes, utf16 code units with surrogate pairs, utf32 one) and are inverse
on every character boundary; both refuse a position inside a character or past
the end. The round-trip law itself is not stated as a theorem, and only
`toByteOffset` is differentially tested — `toCharacter` has no test of its own.

Re-judged with spec, still agrees with a note. Spec: `SpecLsp.ByteOffsetAt`
gives an honest meaning of a column. Column *n* is byte *b* when some prefix of
the line spans *n* units and *b* bytes, and no such prefix means no offset. It
pins `toByteOffset`. It is **weaker than the clause**: it covers one direction
only. `toCharacter` is not pinned, and the round-trip law is still not stated.
A matching spec for `toCharacter` on the same prefix relation would give the
round trip directly.
