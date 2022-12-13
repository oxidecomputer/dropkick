use pgp::SignedPublicKey;
use std::io::Cursor;

lazy_static::lazy_static! {
    pub static ref UBUNTU: SignedPublicKey = read_armored_key(include_str!("ubuntu.asc"));
}

#[cfg(test)]
#[test]
fn doesnt_panic() {
    let _ = &*UBUNTU;
}

fn read_armored_key(armored: &'static str) -> SignedPublicKey {
    pgp::composed::signed_key::parse::from_armor_many(Cursor::new(armored.as_bytes()))
        .unwrap()
        .0
        .next()
        .unwrap()
        .unwrap()
        .into_public()
}
