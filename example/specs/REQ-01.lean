//ricostynha author
-- Formal specification for REQ-01: User Authentication
-- The primary value here: datatypes define the contract.
-- Implementation code MUST match these types exactly.

/-- Non-empty string wrapper. Username can't be blank. -/
structure Username where
  val : String
  nonempty : val.length > 0

/-- Password with minimum strength requirement. -/
structure Password where
  val : String
  min_length : val.length ≥ 8

/-- Credentials bundle — both fields carry their own invariants. -/
structure Credentials where
  username : Username
  password : Password

/-- Authentication token — opaque, non-empty. -/
structure AuthToken where
  val : String
  nonempty : val.length > 0

/-- All ways authentication can fail. Implementation must handle each. -/
inductive AuthError where
  | emptyUsername
  | weakPassword
  | invalidCredentials
  | accountLocked (remainingMinutes : Nat)
  | rateLimited

/-- Login result — the implementation must return exactly this. -/
def LoginResult := Except AuthError AuthToken

/-- Logout is infallible given a token. -/
def LogoutResult := Bool

-- Correctness properties (secondary value — nice to have)

theorem login_rejects_empty_username :
  ∀ (pass : Password),
    -- Can't construct a Username with empty string,
    -- so this is enforced by the type itself.
    ¬ (∃ u : Username, u.val = "") := by
  intro pass h
  obtain ⟨u, hu⟩ := h
  omega_nat_or_simp_at hu u.nonempty

theorem valid_token_nonempty :
  ∀ (t : AuthToken), t.val.length > 0 :=
  fun t => t.nonempty
-- Formal specification for REQ-01: User Authentication
-- The primary value here: datatypes define the contract.
-- Implementation code MUST match these types exactly.

/-- Non-empty string wrapper. Username can't be blank. -/
structure Username where
  val : String
  nonempty : val.length > 0

/-- Password with minimum strength requirement. -/
structure Password where
  val : String
  min_length : val.length ≥ 8

/-- Credentials bundle — both fields carry their own invariants. -/
structure Credentials where
  username : Username
  password : Password

/-- Authentication token — opaque, non-empty. -/
structure AuthToken where
  val : String
  nonempty : val.length > 0

/-- All ways authentication can fail. Implementation must handle each. -/
inductive AuthError where
  | emptyUsername
  | weakPassword
  | invalidCredentials
  | accountLocked (remainingMinutes : Nat)
  | rateLimited

/-- Login result — the implementation must return exactly this. -/
def LoginResult := Except AuthError AuthToken

/-- Logout is infallible given a token. -/
def LogoutResult := Bool

-- Correctness properties (secondary value — nice to have)

theorem login_rejects_empty_username :
  ∀ (pass : Password),
    -- Can't construct a Username with empty string,
    -- so this is enforced by the type itself.
    ¬ (∃ u : Username, u.val = "") := by
  intro pass h
  obtain ⟨u, hu⟩ := h
  omega_nat_or_simp_at hu u.nonempty

theorem valid_token_nonempty :
  ∀ (t : AuthToken), t.val.length > 0 :=
  fun t => t.nonempty
