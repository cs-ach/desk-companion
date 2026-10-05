use std::collections::BTreeMap;
use std::sync::Arc;

use plugin::Secrets;

#[derive(Clone, Default)]
pub struct SecretStore {
    values: Arc<BTreeMap<String, String>>,
}

impl SecretStore {
    pub fn new(secrets: &Secrets) -> Self {
        let values = secrets
            .keys()
            .filter_map(|key| secrets.get(key).map(|v| (key.to_owned(), v.to_owned())))
            .collect();

        Self { values: Arc::new(values) }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn apply(&self, text: &str) -> String {
        if self.values.is_empty() || !text.contains("{{") {
            return text.to_owned();
        }

        let mut out = text.to_owned();
        for (key, value) in self.values.iter() {
            let placeholder = format!("{{{{{key}}}}}");
            if out.contains(&placeholder) {
                out = out.replace(&placeholder, value);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(pairs: &[(&str, &str)]) -> SecretStore {
        SecretStore {
            values: Arc::new(
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ),
        }
    }

    #[test]
    fn a_placeholder_is_replaced() {
        let s = store(&[("refreshToken", "secret-value")]);
        assert_eq!(s.apply("refresh_token={{refreshToken}}"), "refresh_token=secret-value");
    }

    #[test]
    fn every_occurrence_is_replaced() {
        let s = store(&[("token", "abc")]);
        assert_eq!(s.apply("{{token}}/{{token}}"), "abc/abc");
    }

    #[test]
    fn an_unknown_placeholder_is_left_intact() {
        let s = store(&[("token", "abc")]);
        assert_eq!(s.apply("a={{nope}}"), "a={{nope}}");
    }

    #[test]
    fn text_without_placeholders_is_untouched() {
        let s = store(&[("token", "abc")]);
        assert_eq!(s.apply("grant_type=refresh_token"), "grant_type=refresh_token");
    }

    #[test]
    fn an_empty_store_changes_nothing() {
        let s = SecretStore::default();
        assert_eq!(s.apply("{{token}}"), "{{token}}");
        assert!(s.is_empty());
    }
}
