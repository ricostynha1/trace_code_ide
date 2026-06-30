/// Unit tests for auth module

#[cfg(test)]
mod tests {
    use super::super::auth::*;

    #[test]
    fn login_success() {
        let creds = Credentials {
            username: "alice".into(),
            password: "secure_pass_123".into(),
        };
        assert!(login(&creds).is_ok());
    }

    #[test]
    fn login_empty_username_fails() {
        let creds = Credentials {
            username: "".into(),
            password: "secure_pass_123".into(),
        };
        assert!(login(&creds).is_err());
    }

    #[test]
    fn login_weak_password_fails() {
        let creds = Credentials {
            username: "alice".into(),
            password: "short".into(),
        };
        assert!(login(&creds).is_err());
    }

    #[test]
    fn logout_with_valid_token() {
        assert!(logout("token_alice"));
    }
}
