use crate::error::Result;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub email: Option<String>,
    pub role: Option<String>,
}

pub struct JwtValidator {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtValidator {
    pub fn new(jwt_secret: &str) -> Self {
        let decoding_key = DecodingKey::from_secret(jwt_secret.as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["authenticated"]);
        validation.set_required_spec_claims(&["sub", "aud", "exp"]);
        Self {
            decoding_key,
            validation,
        }
    }

    pub fn validate(&self, token: &str) -> Result<Claims> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }
}

pub fn create_test_token(secret: &str, claims: &Claims) -> String {
    let key = EncodingKey::from_secret(secret.as_bytes());
    encode(&Header::new(Algorithm::HS256), claims, &key).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "xpo-test-secret-32-chars-long!!!";

    fn test_claims(exp_offset: i64) -> Claims {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Claims {
            sub: "user-uuid-123".into(),
            aud: "authenticated".into(),
            exp: (now as i64 + exp_offset) as u64,
            iat: now,
            email: Some("test@xpo.sh".into()),
            role: Some("authenticated".into()),
        }
    }

    #[test]
    fn valid_token() {
        let claims = test_claims(3600);
        let token = create_test_token(TEST_SECRET, &claims);
        let validator = JwtValidator::new(TEST_SECRET);
        let result = validator.validate(&token).unwrap();
        assert_eq!(result.sub, "user-uuid-123");
        assert_eq!(result.email.as_deref(), Some("test@xpo.sh"));
    }

    #[test]
    fn expired_token() {
        let claims = test_claims(-3600);
        let token = create_test_token(TEST_SECRET, &claims);
        let validator = JwtValidator::new(TEST_SECRET);
        assert!(validator.validate(&token).is_err());
    }

    #[test]
    fn wrong_audience() {
        let mut claims = test_claims(3600);
        claims.aud = "wrong".into();
        let token = create_test_token(TEST_SECRET, &claims);
        let validator = JwtValidator::new(TEST_SECRET);
        assert!(validator.validate(&token).is_err());
    }

    #[test]
    fn wrong_secret() {
        let claims = test_claims(3600);
        let token = create_test_token(TEST_SECRET, &claims);
        let validator = JwtValidator::new("wrong-secret-that-is-different!!");
        assert!(validator.validate(&token).is_err());
    }

    #[test]
    fn invalid_token_string() {
        let validator = JwtValidator::new(TEST_SECRET);
        assert!(validator.validate("not.a.jwt").is_err());
        assert!(validator.validate("").is_err());
        assert!(validator.validate("garbage").is_err());
    }

    #[test]
    fn tampered_token() {
        let claims = test_claims(3600);
        let mut token = create_test_token(TEST_SECRET, &claims);
        let bytes = unsafe { token.as_bytes_mut() };
        if let Some(b) = bytes.get_mut(20) {
            *b = if *b == b'A' { b'B' } else { b'A' };
        }
        let validator = JwtValidator::new(TEST_SECRET);
        assert!(validator.validate(&token).is_err());
    }
}
