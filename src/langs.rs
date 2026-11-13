//! Reading what language, and what kind, a subtitle file is from its name
//! (`Movie.en.srt`, `Show.S01E02.eng.hi.srt`, `Film.English.forced.srt`).

/// (2-letter code, 3-letter code, English name): the languages subtitle
/// downloaders name files with. Not every ISO language, just the common ones.
const LANGS: &[(&str, &str, &str)] = &[
    ("ar", "ara", "arabic"),
    ("bg", "bul", "bulgarian"),
    ("cs", "ces", "czech"),
    ("da", "dan", "danish"),
    ("de", "ger", "german"),
    ("el", "ell", "greek"),
    ("en", "eng", "english"),
    ("es", "spa", "spanish"),
    ("fa", "fas", "persian"),
    ("fi", "fin", "finnish"),
    ("fr", "fre", "french"),
    ("he", "heb", "hebrew"),
    ("hi", "hin", "hindi"),
    ("hr", "hrv", "croatian"),
    ("hu", "hun", "hungarian"),
    ("id", "ind", "indonesian"),
    ("it", "ita", "italian"),
    ("ja", "jpn", "japanese"),
    ("ko", "kor", "korean"),
    ("nl", "dut", "dutch"),
    ("no", "nor", "norwegian"),
    ("pl", "pol", "polish"),
    ("pt", "por", "portuguese"),
    ("ro", "ron", "romanian"),
    ("ru", "rus", "russian"),
    ("sk", "slk", "slovak"),
    ("sv", "swe", "swedish"),
    ("th", "tha", "thai"),
    ("tr", "tur", "turkish"),
    ("uk", "ukr", "ukrainian"),
    ("vi", "vie", "vietnamese"),
    ("zh", "zho", "chinese"),
];

/// Other 3-letter spellings in use (bibliographic vs terminologic, and the
/// Brazilian Portuguese tag Bazarr uses).
const ALIASES: &[(&str, &str)] = &[
    ("ces", "cs"),
    ("cze", "cs"),
    ("deu", "de"),
    ("ell", "el"),
    ("fas", "fa"),
    ("fra", "fr"),
    ("nld", "nl"),
    ("ron", "ro"),
    ("rum", "ro"),
    ("slk", "sk"),
    ("slo", "sk"),
    ("zho", "zh"),
    ("chi", "zh"),
    ("pob", "pt"),
    ("pb", "pt"),
];

/// The 2-letter code for a language written as `en`, `eng`, `English`,
/// `en-US` or `pt_BR`, or `None` if it isn't one we know.
pub fn code2(text: &str) -> Option<&'static str> {
    let t = text.trim().to_lowercase();
    let t = t.split(['-', '_']).next().unwrap_or("");
    LANGS
        .iter()
        .find(|(c2, c3, name)| t == *c2 || t == *c3 || t == *name)
        .map(|(c2, _, _)| *c2)
        .or_else(|| ALIASES.iter().find(|(a, _)| t == *a).map(|(_, c2)| *c2))
        .and_then(|c| LANGS.iter().find(|(c2, ..)| *c2 == c).map(|(c2, ..)| *c2))
}

/// What a subtitle file's name says about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameInfo {
    /// The language tag in the name, if there is one.
    pub lang: Option<&'static str>,
    /// Only the lines in a foreign language (`.forced.`).
    pub forced: bool,
    /// Made for the hard of hearing: sound descriptions included (`.hi.`, `.sdh.`, `.cc.`).
    pub hi: bool,
}

/// Read the tags out of a subtitle's file name (extension and all), given the
/// video's own name (without extension) so that a title like "Her" or "No" is
/// not mistaken for a language tag.
pub fn name_info(file_name: &str, video_stem: &str) -> NameInfo {
    let lower = file_name.to_lowercase();
    let stem = lower.rsplit_once('.').map(|(s, _)| s).unwrap_or(&lower);
    let tags = stem
        .strip_prefix(&video_stem.to_lowercase())
        .unwrap_or(stem);
    let mut info = NameInfo::default();
    // Tags come at the end of the name, after the video's own name; scan from
    // the back and stop at the first thing that isn't a tag.
    for token in tags.rsplit(['.', '_', ' ', '-', '(', ')', '[', ']']) {
        if token.is_empty() {
            continue;
        }
        match token {
            "forced" => info.forced = true,
            "hi" | "sdh" | "cc" => info.hi = true,
            _ => {
                if let Some(c) = code2(token) {
                    info.lang.get_or_insert(c);
                } else {
                    break;
                }
            }
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_names_and_variants_all_resolve() {
        assert_eq!(code2("en"), Some("en"));
        assert_eq!(code2("ENG"), Some("en"));
        assert_eq!(code2("English"), Some("en"));
        assert_eq!(code2("pt-BR"), Some("pt"));
        assert_eq!(code2("pob"), Some("pt"));
        assert_eq!(code2("fra"), Some("fr"));
        assert_eq!(code2("deu"), Some("de"));
        assert_eq!(code2("xx"), None);
        assert_eq!(code2(""), None);
    }

    #[test]
    fn reads_language_and_kind_from_a_file_name() {
        let i = name_info("Movie (2021).en.srt", "Movie (2021)");
        assert_eq!((i.lang, i.forced, i.hi), (Some("en"), false, false));
        let i = name_info("Show.S01E02.eng.hi.srt", "Show.S01E02");
        assert_eq!((i.lang, i.forced, i.hi), (Some("en"), false, true));
        let i = name_info("Film.French.forced.srt", "Film");
        assert_eq!((i.lang, i.forced, i.hi), (Some("fr"), true, false));
        let i = name_info("Movie.sdh.en.srt", "Movie");
        assert_eq!((i.lang, i.hi), (Some("en"), true));
    }

    #[test]
    fn a_name_with_no_tags_has_none() {
        assert_eq!(name_info("Movie.srt", "Movie"), NameInfo::default());
        // A title that is (or ends in) a language word is not a tag.
        assert_eq!(
            name_info("The English Patient.srt", "The English Patient").lang,
            None
        );
        assert_eq!(name_info("No.srt", "No").lang, None);
        assert_eq!(name_info("No.fr.srt", "No").lang, Some("fr"));
    }
}
