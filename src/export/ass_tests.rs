use super::*;

fn cue(start_ms: i64, end_ms: i64, text: &str) -> Cue {
    Cue {
        start_ms,
        end_ms,
        text: text.into(),
    }
}

fn events(ass: &str) -> Vec<&str> {
    ass.lines().filter(|l| l.starts_with("Dialogue:")).collect()
}

fn plain(cues: &[Cue], window: (i64, i64)) -> Option<String> {
    build_ass(cues, window, 0, &TextLook::default(), None, 100)
}

#[test]
fn the_default_look_rests_on_the_bottom_in_the_288_line_space() {
    let ass = plain(&[cue(1000, 3000, "Hello there")], (0, 5000)).unwrap();
    assert!(ass.contains("PlayResX: 512") && ass.contains("PlayResY: 288"));
    assert!(ass.contains("Style: Default,Arial,16,"));
    let e = events(&ass);
    assert_eq!(e.len(), 1);
    assert!(e[0].starts_with("Dialogue: 0,0:00:01.00,0:00:03.00,"));
    assert!(e[0].contains("{\\an2\\pos(256.0,278.0)}Hello there"));
}

#[test]
fn lines_outside_the_window_are_left_out_and_none_means_nothing_to_burn() {
    let cues = [cue(0, 500, "early"), cue(9000, 9500, "late")];
    assert!(plain(&cues, (1000, 5000)).is_none());
    assert!(plain(&[], (0, 1000)).is_none());
    // a line with nothing but markers has no words
    assert!(plain(&[cue(0, 900, " * ")], (0, 1000)).is_none());
}

#[test]
fn times_are_the_windows_own_and_the_sync_offset_applies() {
    let ass = build_ass(
        &[cue(10_000, 12_000, "x")],
        (9_000, 14_000),
        500,
        &TextLook::default(),
        None,
        100,
    )
    .unwrap();
    // 10.0s + 0.5s offset - 9.0s window start = 1.5s
    assert!(events(&ass)[0].contains("0:00:01.50,0:00:03.50"));
}

#[test]
fn a_position_centres_the_text_on_that_spot() {
    let look = TextLook {
        x: Some(0.25),
        y: Some(0.5),
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &look, None, 100).unwrap();
    assert!(events(&ass)[0].contains("{\\an5\\pos(128.0,144.0)}hi"));
    // out-of-range values stay inside the picture
    let wild = TextLook {
        x: Some(7.0),
        y: Some(-3.0),
        align: TextAlign::Right,
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &wild, None, 100).unwrap();
    assert!(events(&ass)[0].contains("{\\an6\\pos(512.0,0.0)}hi"));
}

#[test]
fn presets_set_the_colours_and_the_size_and_a_look_overrides_them() {
    let block = TextLook {
        preset: Some("block".into()),
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &block, None, 100).unwrap();
    // yellow FFE600 is BGR 00E6FF in a style line; bold is -1; size 36
    assert!(ass.contains("Style: Default,Arial,36,&H0000E6FF,"));
    assert!(ass.contains(",-1,0,0,0,"));
    let tinted = TextLook {
        preset: Some("block".into()),
        color: Some("#00ff00".into()),
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &tinted, None, 200).unwrap();
    assert!(ass.contains("Style: Default,Arial,72,&H0000FF00,"));
    // an unknown preset and a bad colour fall back rather than fail
    let odd = TextLook {
        preset: Some("nope".into()),
        color: Some("red".into()),
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &odd, None, 100).unwrap();
    assert!(ass.contains("Style: Default,Arial,16,&H00FFFFFF,"));
    assert_eq!(parse_hex_color("#FFE600"), Some([255, 230, 0]));
    assert_eq!(parse_hex_color("FFE60"), None);
    assert_eq!(parse_hex_color("#GGGGGG"), None);
}

#[test]
fn the_box_preset_draws_a_bar_not_an_outline() {
    let look = TextLook {
        preset: Some("box".into()),
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &look, None, 100).unwrap();
    // BorderStyle 3 is the opaque box
    assert!(ass.contains(",3,3.0,0.0,2,"));
}

#[test]
fn a_font_family_that_would_break_the_style_line_is_not_used() {
    let mk = |f: Option<&str>| {
        build_ass(
            &[cue(0, 1000, "hi")],
            (0, 1000),
            0,
            &TextLook::default(),
            f,
            100,
        )
        .unwrap()
    };
    assert!(mk(Some("Bangers")).contains("Style: Default,Bangers,"));
    assert!(mk(Some("Evil,Name")).contains("Style: Default,Arial,"));
    assert!(mk(Some("")).contains("Style: Default,Arial,"));
}

#[test]
fn starred_words_are_drawn_larger_and_the_stars_are_not_shown() {
    let ass = plain(
        &[cue(0, 2000, "Well, this place is *oddity* indeed")],
        (0, 2000),
    )
    .unwrap();
    let e = events(&ass)[0];
    // 16 * 1.5 = 24 for the emphasised word, back to 16 after it
    assert!(e.contains("is {\\fs24}oddity {\\fs16}indeed"), "{e}");
    assert!(!e.contains('*'));
    // two starred words share the emphasis
    let ass = plain(&[cue(0, 2000, "*two words* then")], (0, 2000)).unwrap();
    assert!(events(&ass)[0].contains("{\\fs24}two words {\\fs16}then"));
}

#[test]
fn braces_and_backslashes_in_the_text_cannot_inject_tags() {
    let ass = plain(&[cue(0, 1000, "{\\an9}evil \\N text")], (0, 1000)).unwrap();
    let e = events(&ass)[0];
    assert!(e.ends_with("(/an9)evil /N text"), "{e}");
    assert_eq!(e.matches("\\an").count(), 1);
}

#[test]
fn lines_of_the_text_stay_lines() {
    let ass = plain(&[cue(0, 1000, "one\ntwo")], (0, 1000)).unwrap();
    assert!(events(&ass)[0].ends_with("one\\Ntwo"));
}

fn reveal_look(anim: TextAnim) -> TextLook {
    TextLook {
        reveal: TextReveal::Word,
        anim,
        ..Default::default()
    }
}

/// The `\t(start,..` time of every hidden-then-shown word, in order.
fn reveal_times(e: &str) -> Vec<i64> {
    e.split("\\t(")
        .skip(1)
        .map(|s| s.split(',').next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn words_appear_one_after_another_across_most_of_the_line() {
    let ass = build_ass(
        &[cue(0, 4000, "aaaa bbbb cccc dddd")],
        (0, 4000),
        0,
        &reveal_look(TextAnim::None),
        None,
        100,
    )
    .unwrap();
    let e = events(&ass)[0];
    // the first word is there from the start; the others come in at
    // 0.8 * 4000 * (4/16, 8/16, 12/16) = 800, 1600, 2400 ms
    assert!(e.contains("{\\alpha&H00&\\fscx100\\fscy100}aaaa"), "{e}");
    assert_eq!(reveal_times(e), vec![800, 1600, 2400]);
    assert!(e.contains("{\\alpha&HFF&\\t(800,801,\\alpha&H00&)}bbbb"));
}

#[test]
fn a_line_already_under_way_shows_its_earlier_words_at_once() {
    // The clip starts 2.0s into a 4s line of four equal words.
    let ass = build_ass(
        &[cue(0, 4000, "aaaa bbbb cccc dddd")],
        (2000, 4000),
        0,
        &reveal_look(TextAnim::None),
        None,
        100,
    )
    .unwrap();
    let e = events(&ass)[0];
    assert!(e.contains("0:00:00.00,0:00:02.00"));
    // cccc is said at 1600 and dddd at 2400 (the line's own time); the event
    // starts at 2000, so only dddd is still to come, 400 ms in
    assert_eq!(reveal_times(e), vec![400]);
}

#[test]
fn entrances_fade_pop_or_slide() {
    let one = |look: &TextLook| {
        build_ass(&[cue(500, 3000, "hi there")], (0, 4000), 0, look, None, 100).unwrap()
    };
    let fade = one(&TextLook {
        anim: TextAnim::Fade,
        ..Default::default()
    });
    assert!(events(&fade)[0].contains("\\fad(180,0)"));
    let pop = one(&TextLook {
        anim: TextAnim::Pop,
        ..Default::default()
    });
    assert!(events(&pop)[0].contains("\\fscx70\\fscy70\\t(0,180,\\fscx100\\fscy100)"));
    let up = one(&TextLook {
        anim: TextAnim::SlideUp,
        ..Default::default()
    });
    assert!(events(&up)[0].contains("\\move(256.0,302.0,256.0,278.0,0,180)"));
    let left = one(&TextLook {
        anim: TextAnim::SlideLeft,
        ..Default::default()
    });
    assert!(events(&left)[0].contains("\\move(328.0,278.0,256.0,278.0,0,180)"));
    // per word, fade and pop animate each word and the line itself stays put
    let words = one(&reveal_look(TextAnim::Pop));
    assert!(events(&words)[0].contains("\\fscx60\\fscy60\\t("));
    assert!(!events(&words)[0].contains("\\fad("));
}

#[test]
fn a_line_that_began_before_the_clip_does_not_animate_in() {
    let ass = build_ass(
        &[cue(0, 3000, "hi")],
        (1000, 4000),
        0,
        &TextLook {
            anim: TextAnim::SlideUp,
            ..Default::default()
        },
        None,
        100,
    )
    .unwrap();
    let e = events(&ass)[0];
    assert!(e.contains("\\pos(") && !e.contains("\\move("));
}

#[test]
fn times_are_written_in_centiseconds() {
    assert_eq!(ass_time(0), "0:00:00.00");
    assert_eq!(ass_time(61_234), "0:01:01.23");
    assert_eq!(ass_time(3_600_000), "1:00:00.00");
    assert_eq!(ass_time(-5), "0:00:00.00");
}
