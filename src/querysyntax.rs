//! Search operators, in the style of a web search engine.
//!
//! ```text
//! show:"Breaking Bad" ep:pilot        only that show's episodes titled like "pilot"
//! -show:friends                       everything except that show
//! movie:                              only movies         (ep: = only episodes)
//! library:Anime season:2              a library, a season number
//! "i am the one" -walter              dialogue (or title) text, minus a word
//! ```
//!
//! Whatever is left after the operators are taken out is the text to search
//! for: dialogue in Dialogue mode, titles in Title mode. A `key:` that isn't a
//! known operator is just text (dialogue is full of colons), and a quoted
//! phrase is always text, so a line that really starts with `-` or contains
//! `show:` can still be searched by putting it in quotes.

use serde::Serialize;

/// What an operator looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// `show:` the show's name contains the value.
    Show,
    /// `ep:` / `episode:` an episode whose title contains the value (no value: any episode).
    Episode,
    /// `movie:` a movie whose title contains the value (no value: any movie).
    Movie,
    /// `library:` / `lib:` the library has exactly this name (any case).
    Library,
    /// `season:` the season number.
    Season,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Filter {
    pub field: Field,
    /// Empty only for `ep:` / `movie:` without a value.
    pub value: String,
    /// `-show:x`: the opposite.
    pub negate: bool,
}

impl Filter {
    /// How the UI shows it back to the person who typed it.
    pub fn describe(&self) -> String {
        let name = match self.field {
            Field::Show => "show",
            Field::Episode => "episode",
            Field::Movie => "movie",
            Field::Library => "library",
            Field::Season => "season",
        };
        let not = if self.negate { "not " } else { "" };
        if self.value.is_empty() {
            let only = match self.field {
                Field::Episode => "episodes",
                _ => "movies",
            };
            return if self.negate {
                format!("not {only}")
            } else {
                format!("{only} only")
            };
        }
        format!("{not}{name}: {}", self.value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ParsedQuery {
    /// The text to search for, operators and exclusions removed.
    pub text: String,
    /// `-word`: results must not contain these.
    pub excluded: Vec<String>,
    pub filters: Vec<Filter>,
}

impl ParsedQuery {
    /// Anything beyond plain text?
    pub fn has_operators(&self) -> bool {
        !self.filters.is_empty() || !self.excluded.is_empty()
    }

    /// One short phrase per operator, for showing what was understood.
    pub fn describe(&self) -> Vec<String> {
        let mut out: Vec<String> = self.filters.iter().map(Filter::describe).collect();
        out.extend(self.excluded.iter().map(|w| format!("without “{w}”")));
        out
    }
}

fn field_for(key: &str) -> Option<Field> {
    match key.to_ascii_lowercase().as_str() {
        "show" => Some(Field::Show),
        "ep" | "episode" => Some(Field::Episode),
        "movie" => Some(Field::Movie),
        "library" | "lib" => Some(Field::Library),
        "season" => Some(Field::Season),
        _ => None,
    }
}

/// A value: a `"quoted phrase"` (the quotes are dropped; an unclosed quote runs
/// to the end) or everything up to the next space.
fn read_value(chars: &[char], i: &mut usize) -> String {
    let mut out = String::new();
    if *i < chars.len() && chars[*i] == '"' {
        *i += 1;
        while *i < chars.len() && chars[*i] != '"' {
            out.push(chars[*i]);
            *i += 1;
        }
        if *i < chars.len() {
            *i += 1; // the closing quote
        }
    } else {
        while *i < chars.len() && !chars[*i].is_whitespace() {
            out.push(chars[*i]);
            *i += 1;
        }
    }
    out.trim().to_string()
}

pub fn parse(input: &str) -> ParsedQuery {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut parsed = ParsedQuery::default();
    let mut text: Vec<String> = Vec::new();

    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let token_start = i;
        // A leading `-` negates, but only when something follows it directly.
        let mut negate = false;
        if chars[i] == '-' && i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
            negate = true;
            i += 1;
        }
        // `key:` with a known key?
        let key_start = i;
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        let field = if j > key_start && j < chars.len() && chars[j] == ':' {
            field_for(&chars[key_start..j].iter().collect::<String>())
        } else {
            None
        };
        if let Some(field) = field {
            i = j + 1;
            let value = read_value(&chars, &mut i);
            match field {
                Field::Season => match value.parse::<i64>() {
                    Ok(n) => parsed.filters.push(Filter {
                        field,
                        value: n.to_string(),
                        negate,
                    }),
                    // `season:two` isn't an operator: keep it as the text it was.
                    Err(_) => text.push(chars[token_start..i].iter().collect()),
                },
                Field::Show | Field::Library if value.is_empty() => {
                    // `show:` with nothing after it filters nothing.
                }
                _ => parsed.filters.push(Filter {
                    field,
                    value,
                    negate,
                }),
            }
            continue;
        }
        // Plain text (or an excluded word).
        let word = read_value(&chars, &mut i);
        if word.is_empty() {
            continue;
        }
        if negate {
            parsed.excluded.push(word);
        } else {
            text.push(word);
        }
    }
    parsed.text = text.join(" ");
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(field: Field, value: &str, negate: bool) -> Filter {
        Filter {
            field,
            value: value.into(),
            negate,
        }
    }

    #[test]
    fn plain_text_is_just_text() {
        let p = parse("I am the one who knocks");
        assert_eq!(p.text, "I am the one who knocks");
        assert!(!p.has_operators());
    }

    #[test]
    fn show_ep_and_movie_operators_with_and_without_quotes() {
        let p = parse(r#"show:"Breaking Bad" ep:pilot movie:dune"#);
        assert_eq!(p.text, "");
        assert_eq!(
            p.filters,
            [
                f(Field::Show, "Breaking Bad", false),
                f(Field::Episode, "pilot", false),
                f(Field::Movie, "dune", false),
            ]
        );
    }

    #[test]
    fn a_minus_excludes() {
        let p = parse("knock -show:friends -ep:\"the pilot\" -movie:dune");
        assert_eq!(p.text, "knock");
        assert_eq!(
            p.filters,
            [
                f(Field::Show, "friends", true),
                f(Field::Episode, "the pilot", true),
                f(Field::Movie, "dune", true),
            ]
        );
    }

    #[test]
    fn operators_can_surround_the_text_in_any_order() {
        let p = parse(r#"show:office i am "the one" ep:pilot who knocks"#);
        assert_eq!(p.text, "i am the one who knocks");
        assert_eq!(p.filters.len(), 2);
    }

    #[test]
    fn excluded_words_and_phrases() {
        let p = parse(r#"knock -walter -"say my name""#);
        assert_eq!(p.text, "knock");
        assert_eq!(p.excluded, ["walter", "say my name"]);
        assert!(p.has_operators());
    }

    #[test]
    fn a_bare_kind_operator_means_only_that_kind() {
        let p = parse("movie: knock");
        assert_eq!(p.filters, [f(Field::Movie, "", false)]);
        assert_eq!(p.text, "knock");
        let p = parse("-ep: x");
        assert_eq!(p.filters, [f(Field::Episode, "", true)]);
        assert_eq!(p.filters[0].describe(), "not episodes");
        assert_eq!(parse("ep:").filters[0].describe(), "episodes only");
    }

    #[test]
    fn aliases_and_case() {
        let p = parse("EP:Pilot Episode:two LIB:Anime library:TV");
        assert_eq!(
            p.filters,
            [
                f(Field::Episode, "Pilot", false),
                f(Field::Episode, "two", false),
                f(Field::Library, "Anime", false),
                f(Field::Library, "TV", false),
            ]
        );
    }

    #[test]
    fn season_needs_a_number() {
        assert_eq!(parse("season:2").filters, [f(Field::Season, "2", false)]);
        assert_eq!(parse("-season:02").filters, [f(Field::Season, "2", true)]);
        // not a number: it's just text
        let p = parse("season:two finale");
        assert!(p.filters.is_empty());
        assert_eq!(p.text, "season:two finale");
    }

    #[test]
    fn unknown_keys_and_colons_in_dialogue_are_text() {
        assert_eq!(parse("note: hello there").text, "note: hello there");
        assert_eq!(parse("what time: is it").text, "what time: is it");
        assert_eq!(parse("http://x.y").text, "http://x.y");
        assert!(parse("12:30 pm").filters.is_empty());
    }

    #[test]
    fn a_dash_only_negates_when_something_follows_it() {
        assert_eq!(parse("well - hello").text, "well - hello");
        assert_eq!(parse("-").text, "-");
        assert!(parse("well - hello").excluded.is_empty());
    }

    #[test]
    fn quotes_keep_operator_looking_text_as_text() {
        let p = parse(r#""show:me the money" "-5 degrees""#);
        assert_eq!(p.text, "show:me the money -5 degrees");
        assert!(!p.has_operators());
    }

    #[test]
    fn an_unclosed_quote_runs_to_the_end() {
        let p = parse(r#"show:"Breaking Ba"#);
        assert_eq!(p.filters, [f(Field::Show, "Breaking Ba", false)]);
        let p = parse(r#"hello "big wor"#);
        assert_eq!(p.text, "hello big wor");
    }

    #[test]
    fn empty_show_or_library_value_is_ignored() {
        assert!(parse("show: knock").filters.is_empty());
        assert_eq!(parse("show: knock").text, "knock");
        assert!(parse("library:").filters.is_empty());
    }

    #[test]
    fn describe_reads_back_what_was_understood() {
        let p = parse(r#"knock show:"Breaking Bad" -ep:pilot -walter season:2 movie:"#);
        assert_eq!(
            p.describe(),
            [
                "show: Breaking Bad",
                "not episode: pilot",
                "season: 2",
                "movies only",
                "without “walter”"
            ]
        );
    }

    #[test]
    fn empty_and_whitespace_input() {
        assert_eq!(parse(""), ParsedQuery::default());
        assert_eq!(parse("   "), ParsedQuery::default());
    }
}
