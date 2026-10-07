---
id: REQ-AUTH-03
title: User authentication
refines: [REQ-AUTH]
decomposition: complete
status: approved
clauses:
  post: returns a token iff the credentials are valid
  err: every failure maps to exactly one AuthError variant
  log: failed attempts are logged
---
The system shall authenticate users via username and password.
