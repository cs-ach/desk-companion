use base64::Engine;
use jsrt::rquickjs::{self, Ctx, Function, Value, function::Opt};
use rand::{RngExt};
use sha2::{Digest, Sha256};

const VERIFIER_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
const DEFAULT_LENGTH: usize = 64;

pub fn random_string<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(ctx.clone(), |length: Opt<u32>| {
        let length = length.0.unwrap_or(DEFAULT_LENGTH as u32).clamp(43, 128) as usize;
        let mut rng = rand::rng();

        (0..length)
            .map(|_| {
                let i = rng.random_range(0..VERIFIER_ALPHABET.len());
                VERIFIER_ALPHABET[i] as char
            })
            .collect::<String>()
    })?;

    Ok(function.into_value())
}

pub fn sha256_base64url<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Value<'js>> {
    let function = Function::new(ctx.clone(), |text: String| {
        let digest = Sha256::digest(text.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    })?;

    Ok(function.into_value())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_the_rfc_7636_example() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn a_verifier_is_url_safe_and_long_enough() {
        let mut rng = rand::rng();
        let s: String = (0..64)
            .map(|_| VERIFIER_ALPHABET[rng.random_range(0..VERIFIER_ALPHABET.len())] as char)
            .collect();

        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| VERIFIER_ALPHABET.contains(&(c as u8))));
    }
}
