// User authentication module
pub struct Credentials {
    pub username: String,
    pub password: String,
}

pub fn login(creds: &Credentials) -> Result<String, AuthError> {
    if creds.username.is_empty() {
        return Err(AuthError::EmptyUsername);
    }
    if creds.password.len() < 8 {
        return Err(AuthError::WeakPassword);
    }
    // In real impl: check against DB
    Ok(format!("token_{}", creds.username))
}

pub fn logout(token: &str) -> bool {
    !token.is_empty()
}

pub enum AuthError {
    EmptyUsername,
    WeakPassword,
    InvalidCredentials,
}
