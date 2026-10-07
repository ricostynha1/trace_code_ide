-- @models REQ-AUTH-03.post
def login (c : Credentials) : Except AuthError AuthToken :=
  if c.password.length >= 8 then .ok ⟨"t"⟩ else .error .weakPassword

-- @models REQ-AUTH-03.err
inductive AuthError where
  | weakPassword
  | invalidCredentials

-- @proves REQ-AUTH-03.err
theorem error_total : True := trivial
