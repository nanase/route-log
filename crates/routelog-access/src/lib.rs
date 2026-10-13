//! Cloudflare Access が発行したトークン（JWT）の検証。
//!
//! 呼び出し元を締め出すのは、経路の前に立つ Access のポリシーである。ここは同じトークンを
//! もう一度、独立に確かめる。ポリシーが外れたり設定を誤ったりしたときに、書き込みの経路へ
//! 要求が素通りしないためである。
//!
//! 入出力を持たないので、鍵の取得は呼び出し側が行う。

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::sha2::Sha256;
use rsa::signature::Verifier;
use rsa::{BigUint, RsaPublicKey};
use serde::Deserialize;

/// トークンの期限と、この検証をする側の時計のずれとして許す秒数。
const CLOCK_SKEW_SECONDS: i64 = 60;

/// Access の公開鍵 1 つ。`https://<チームのドメイン>/cdn-cgi/access/certs` が返すもの。
#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    pub kid: String,
    /// 法（base64url）。
    pub n: String,
    /// 公開指数（base64url）。
    pub e: String,
}

/// Access の公開鍵の一式。
#[derive(Debug, Clone, Deserialize)]
pub struct Keys {
    pub keys: Vec<Jwk>,
}

/// 検証を通ったトークンの持ち主。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// 人がログインした。メールアドレスを持つ。
    User(String),
    /// サービストークン。クライアント ID を持つ。
    Service(String),
}

/// トークンを拒んだ理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("トークンの形が正しくありません")]
    Malformed,
    #[error("署名の方式が RS256 ではありません")]
    Algorithm,
    #[error("発行者が違います")]
    Issuer,
    #[error("宛先が違います")]
    Audience,
    #[error("期限が切れています")]
    Expired,
    #[error("まだ有効になっていません")]
    NotYetValid,
    #[error("署名した鍵が見つかりません")]
    UnknownKey,
    #[error("署名が合いません")]
    Signature,
    #[error("持ち主が分かりません")]
    NoIdentity,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

/// `aud` は文字列 1 つのことも、配列のこともある。
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    exp: i64,
    nbf: Option<i64>,
    email: Option<String>,
    common_name: Option<String>,
}

fn decode(segment: &str) -> Result<Vec<u8>, Rejected> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| Rejected::Malformed)
}

fn json<T: for<'de> Deserialize<'de>>(segment: &str) -> Result<T, Rejected> {
    serde_json::from_slice(&decode(segment)?).map_err(|_| Rejected::Malformed)
}

/// トークンを検証し、持ち主を返す。
///
/// - `team_domain`: Access のチームのドメイン。`<team>.cloudflareaccess.com` の形
/// - `audience`: 経路の前にある Access のアプリケーションの `aud` タグ
/// - `now_seconds`: いまの時刻（Unix 秒）
pub fn verify(
    token: &str,
    keys: &Keys,
    team_domain: &str,
    audience: &str,
    now_seconds: i64,
) -> Result<Identity, Rejected> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Rejected::Malformed);
    };
    let Header { alg, kid } = json(header)?;
    if alg != "RS256" {
        return Err(Rejected::Algorithm);
    }
    let claims: Claims = json(payload)?;
    if claims.iss != format!("https://{team_domain}") {
        return Err(Rejected::Issuer);
    }
    let addressed = match &claims.aud {
        Audience::One(one) => one == audience,
        Audience::Many(many) => many.iter().any(|one| one == audience),
    };
    if !addressed {
        return Err(Rejected::Audience);
    }
    if now_seconds > claims.exp + CLOCK_SKEW_SECONDS {
        return Err(Rejected::Expired);
    }
    if claims
        .nbf
        .is_some_and(|nbf| now_seconds < nbf - CLOCK_SKEW_SECONDS)
    {
        return Err(Rejected::NotYetValid);
    }

    let jwk = keys
        .keys
        .iter()
        .find(|key| key.kid == kid)
        .ok_or(Rejected::UnknownKey)?;
    let number = |text: &str| decode(text).map(|bytes| BigUint::from_bytes_be(&bytes));
    let public =
        RsaPublicKey::new(number(&jwk.n)?, number(&jwk.e)?).map_err(|_| Rejected::UnknownKey)?;
    let signature =
        Signature::try_from(decode(signature)?.as_slice()).map_err(|_| Rejected::Signature)?;
    let signed = &token[..header.len() + 1 + payload.len()];
    VerifyingKey::<Sha256>::new(public)
        .verify(signed.as_bytes(), &signature)
        .map_err(|_| Rejected::Signature)?;

    let filled = |value: Option<String>| value.filter(|v| !v.is_empty());
    match (filled(claims.email), filled(claims.common_name)) {
        (Some(email), _) => Ok(Identity::User(email)),
        (None, Some(client)) => Ok(Identity::Service(client)),
        (None, None) => Err(Rejected::NoIdentity),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use rsa::RsaPrivateKey;
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::traits::PublicKeyParts;

    use super::*;

    const TEAM: &str = "example.cloudflareaccess.com";
    const AUD: &str = "aud-of-this-application";
    const NOW: i64 = 1_800_000_000;

    /// テスト用の鍵。作るのに時間がかかるので、1 度だけ作る。
    fn private() -> &'static RsaPrivateKey {
        static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
        KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap())
    }

    fn keys() -> Keys {
        let public = private().to_public_key();
        Keys {
            keys: vec![Jwk {
                kid: "key-1".into(),
                n: URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
                e: URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
            }],
        }
    }

    fn token(header: serde_json::Value, claims: serde_json::Value) -> String {
        let encode = |value: &serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
        let signed = format!("{}.{}", encode(&header), encode(&claims));
        let signature = SigningKey::<Sha256>::new(private().clone()).sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    fn header() -> serde_json::Value {
        serde_json::json!({ "alg": "RS256", "kid": "key-1", "typ": "JWT" })
    }

    fn claims() -> serde_json::Value {
        serde_json::json!({
            "iss": format!("https://{TEAM}"),
            "aud": [AUD],
            "exp": NOW + 600,
            "nbf": NOW - 10,
            "email": "rider@example.com",
        })
    }

    fn with(change: impl FnOnce(&mut serde_json::Value)) -> serde_json::Value {
        let mut claims = claims();
        change(&mut claims);
        claims
    }

    fn check(token: &str) -> Result<Identity, Rejected> {
        verify(token, &keys(), TEAM, AUD, NOW)
    }

    #[test]
    fn a_token_signed_by_access_names_its_user() {
        let identity = check(&token(header(), claims()));
        assert_eq!(identity, Ok(Identity::User("rider@example.com".into())));
    }

    #[test]
    fn a_service_token_has_a_client_id_instead_of_an_email() {
        let claims = with(|c| {
            c["email"] = serde_json::Value::Null;
            c["common_name"] = "client-id.access".into();
            c["aud"] = AUD.into();
        });
        assert_eq!(
            check(&token(header(), claims)),
            Ok(Identity::Service("client-id.access".into()))
        );
    }

    #[test]
    fn a_token_for_another_application_or_team_is_refused() {
        let other_aud = with(|c| c["aud"] = serde_json::json!(["someone-else"]));
        assert_eq!(check(&token(header(), other_aud)), Err(Rejected::Audience));
        let other_team = with(|c| c["iss"] = "https://evil.cloudflareaccess.com".into());
        assert_eq!(check(&token(header(), other_team)), Err(Rejected::Issuer));
    }

    #[test]
    fn a_token_outside_its_time_is_refused_with_a_minute_of_slack() {
        let just_expired = with(|c| c["exp"] = (NOW - 30).into());
        assert!(check(&token(header(), just_expired)).is_ok());
        let expired = with(|c| c["exp"] = (NOW - 61).into());
        assert_eq!(check(&token(header(), expired)), Err(Rejected::Expired));
        let early = with(|c| c["nbf"] = (NOW + 61).into());
        assert_eq!(check(&token(header(), early)), Err(Rejected::NotYetValid));
    }

    #[test]
    fn a_changed_payload_does_not_match_the_signature() {
        let good = token(header(), claims());
        let forged_claims = with(|c| c["email"] = "attacker@example.com".into());
        let forged_payload = URL_SAFE_NO_PAD.encode(forged_claims.to_string());
        let mut parts: Vec<&str> = good.split('.').collect();
        parts[1] = &forged_payload;
        assert_eq!(check(&parts.join(".")), Err(Rejected::Signature));
    }

    #[test]
    fn a_token_signed_by_a_key_access_does_not_have_is_refused() {
        let mut header = header();
        header["kid"] = "key-2".into();
        assert_eq!(check(&token(header, claims())), Err(Rejected::UnknownKey));
    }

    #[test]
    fn an_unsigned_token_is_refused() {
        let mut header = header();
        header["alg"] = "none".into();
        assert_eq!(check(&token(header, claims())), Err(Rejected::Algorithm));
    }

    #[test]
    fn a_token_without_an_owner_is_refused() {
        let nobody = with(|c| c["email"] = "".into());
        assert_eq!(check(&token(header(), nobody)), Err(Rejected::NoIdentity));
    }

    #[test]
    fn junk_is_refused() {
        for junk in ["", "a.b", "a.b.c.d", "!!!.???.***"] {
            assert_eq!(check(junk), Err(Rejected::Malformed), "{junk}");
        }
    }
}
