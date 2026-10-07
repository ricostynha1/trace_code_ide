/// Authentication service.
pub struct AuthService;

impl AuthService {
    // @implements REQ-AUTH-03.post exclusive
    pub fn login(&self, password: &str) -> bool {
        password.len() >= 8
    }

    // @implements REQ-AUTH-03.err
    // @partial reason="account lockout not handled yet"
    pub fn classify(&self, ok: bool) -> &'static str {
        if ok { "ok" } else { "weak" }
    }
}

// @exempt REQ-AUTH-03.log reason="side effect, not in the pure model" by=ana
pub fn audit_log(_msg: &str) {}
