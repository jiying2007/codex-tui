use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en", alias = "en-US", alias = "en_US")]
    English,
    #[serde(
        rename = "zh-CN",
        alias = "zh",
        alias = "zh-cn",
        alias = "zh_CN",
        alias = "zh-Hans",
        alias = "zh_Hans"
    )]
    SimplifiedChinese,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiLanguage {
    #[default]
    English,
    SimplifiedChinese,
}

impl LanguagePreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
        }
    }

    pub fn resolve(self) -> UiLanguage {
        match self {
            Self::English => UiLanguage::English,
            Self::SimplifiedChinese => UiLanguage::SimplifiedChinese,
            Self::Auto => resolve_auto_locale(
                ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .into_iter()
                    .filter_map(|name| std::env::var(name).ok()),
            ),
        }
    }

    pub fn resolve_from_locale_values<'a>(
        self,
        values: impl IntoIterator<Item = &'a str>,
    ) -> UiLanguage {
        match self {
            Self::English => UiLanguage::English,
            Self::SimplifiedChinese => UiLanguage::SimplifiedChinese,
            Self::Auto => resolve_auto_locale(values),
        }
    }
}

impl UiLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
        }
    }

    pub const fn is_simplified_chinese(self) -> bool {
        matches!(self, Self::SimplifiedChinese)
    }
}

pub const fn pick<'a>(
    language: UiLanguage,
    english: &'a str,
    simplified_chinese: &'a str,
) -> &'a str {
    match language {
        UiLanguage::English => english,
        UiLanguage::SimplifiedChinese => simplified_chinese,
    }
}

fn resolve_auto_locale<'a>(values: impl IntoIterator<Item = &'a str>) -> UiLanguage {
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        return if locale_is_chinese(value) {
            UiLanguage::SimplifiedChinese
        } else {
            UiLanguage::English
        };
    }
    UiLanguage::English
}

fn locale_is_chinese(value: &str) -> bool {
    let normalized = value.trim().replace('_', "-").to_ascii_lowercase();
    normalized == "zh" || normalized.starts_with("zh-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_locale_recognizes_common_chinese_spellings() {
        for locale in ["zh", "zh_CN.UTF-8", "zh-CN", "zh_Hans_CN.UTF-8"] {
            assert_eq!(
                LanguagePreference::Auto.resolve_from_locale_values([locale]),
                UiLanguage::SimplifiedChinese,
                "{locale}"
            );
        }
    }

    #[test]
    fn auto_locale_keeps_non_chinese_and_missing_locales_english() {
        assert_eq!(
            LanguagePreference::Auto.resolve_from_locale_values(["en_US.UTF-8"]),
            UiLanguage::English
        );
        assert_eq!(
            LanguagePreference::Auto.resolve_from_locale_values(["C.UTF-8"]),
            UiLanguage::English
        );
        assert_eq!(
            LanguagePreference::Auto.resolve_from_locale_values(std::iter::empty::<&str>()),
            UiLanguage::English
        );
    }

    #[test]
    fn explicit_language_overrides_locale() {
        assert_eq!(
            LanguagePreference::English.resolve_from_locale_values(["zh_CN.UTF-8"]),
            UiLanguage::English
        );
        assert_eq!(
            LanguagePreference::SimplifiedChinese.resolve_from_locale_values(["en_US.UTF-8"]),
            UiLanguage::SimplifiedChinese
        );
    }

    #[test]
    fn serde_contract_uses_stable_config_spellings_and_aliases() {
        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct Fixture {
            language: LanguagePreference,
        }

        let encoded = toml::to_string(&Fixture {
            language: LanguagePreference::Auto,
        })
        .expect("serialize");
        assert!(encoded.contains("language = \"auto\""));

        assert_eq!(
            toml::from_str::<Fixture>("language = \"zh_CN\"").expect("alias"),
            Fixture {
                language: LanguagePreference::SimplifiedChinese,
            }
        );
        assert_eq!(
            toml::from_str::<Fixture>("language = \"en\"").expect("english"),
            Fixture {
                language: LanguagePreference::English,
            }
        );
    }
}
