pub fn json() -> String {
    let ver = env!("CARGO_PKG_VERSION");
    format!(
        r#"{{
  "name": "relay",
  "description": "no description",
  "pubkey": "no pubkey",
  "contact": "no contact",
  "supported_nips": [1, 2, 9, 11, 15, 16, 20],
  "software": "private relay",
  "version": "{ver}"
}}"#
    )
}
