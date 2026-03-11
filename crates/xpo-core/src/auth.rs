use crate::error::Result;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
#[cfg(test)]
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub email: Option<String>,
    pub role: Option<String>,
    pub xpo_plan: Option<String>,
    pub xpo_max_tunnels: Option<u64>,
    pub xpo_max_ttl_secs: Option<u64>,
    pub xpo_allow_custom_subdomain: Option<bool>,
}

pub struct JwtValidator {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtValidator {
    pub fn new(key_material: &str) -> Self {
        let (decoding_key, algorithms) = if looks_like_public_pem(key_material) {
            decoding_key_from_public_pem(key_material.as_bytes())
                .unwrap_or_else(|e| panic!("invalid JWT public key PEM: {e}"))
        } else {
            (
                DecodingKey::from_secret(key_material.as_bytes()),
                vec![Algorithm::HS256],
            )
        };

        Self {
            decoding_key,
            validation: validation_for(algorithms),
        }
    }

    pub fn validate(&self, token: &str) -> Result<Claims> {
        let token_data = decode::<Claims>(token, &self.decoding_key, &self.validation)?;
        Ok(token_data.claims)
    }
}

fn validation_for(algorithms: Vec<Algorithm>) -> Validation {
    let mut validation = Validation::new(*algorithms.first().unwrap_or(&Algorithm::HS256));
    validation.algorithms = algorithms;
    validation.set_audience(&["authenticated"]);
    validation.set_required_spec_claims(&["sub", "aud", "exp"]);
    validation
}

fn looks_like_public_pem(key_material: &str) -> bool {
    let trimmed = key_material.trim();
    trimmed.starts_with("-----BEGIN ")
}

fn decoding_key_from_public_pem(
    pem: &[u8],
) -> std::result::Result<(DecodingKey, Vec<Algorithm>), jsonwebtoken::errors::Error> {
    if let Ok(decoding_key) = DecodingKey::from_rsa_pem(pem) {
        return Ok((
            decoding_key,
            vec![
                Algorithm::RS256,
                Algorithm::RS384,
                Algorithm::RS512,
                Algorithm::PS256,
                Algorithm::PS384,
                Algorithm::PS512,
            ],
        ));
    }

    if let Ok(decoding_key) = DecodingKey::from_ec_pem(pem) {
        return Ok((decoding_key, vec![Algorithm::ES256, Algorithm::ES384]));
    }

    DecodingKey::from_ed_pem(pem).map(|decoding_key| (decoding_key, vec![Algorithm::EdDSA]))
}

#[cfg(test)]
pub fn create_test_token(secret: &str, claims: &Claims) -> String {
    let key = EncodingKey::from_secret(secret.as_bytes());
    encode(&Header::new(Algorithm::HS256), claims, &key).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{KeyPair, PKCS_ECDSA_P256_SHA256};

    const TEST_SECRET: &str = "xpo-test-secret-32-chars-long!!!";

    fn generate_ec_keypair() -> (String, String) {
        let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let private_key_pem = key_pair.serialize_pem();
        let public_key_pem = key_pair.public_key_pem();
        (private_key_pem, public_key_pem)
    }

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
            xpo_plan: None,
            xpo_max_tunnels: None,
            xpo_max_ttl_secs: None,
            xpo_allow_custom_subdomain: None,
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
    fn valid_public_key_token() {
        let claims = test_claims(3600);
        let (private_key_pem, public_key_pem) = generate_ec_keypair();
        let key = EncodingKey::from_ec_pem(private_key_pem.as_bytes()).unwrap();
        let token = encode(&Header::new(Algorithm::ES256), &claims, &key).unwrap();
        let validator = JwtValidator::new(&public_key_pem);
        let result = validator.validate(&token).unwrap();
        assert_eq!(result.sub, "user-uuid-123");
    }

    #[test]
    fn public_key_validator_rejects_hs256_token() {
        let claims = test_claims(3600);
        let token = create_test_token(TEST_SECRET, &claims);
        let (_private_key_pem, public_key_pem) = generate_ec_keypair();
        let validator = JwtValidator::new(&public_key_pem);
        assert!(validator.validate(&token).is_err());
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
