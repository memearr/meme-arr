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

fn kf(t_ms: i64, x: f32, y: f32) -> Keyframe {
    Keyframe { t_ms, x, y }
}

fn pathed(path: Vec<Keyframe>) -> TextLook {
    TextLook {
        path,
        ..Default::default()
    }
}

#[test]
fn text_follows_its_keyframes_in_straight_lines_and_holds_at_the_last() {
    let look = pathed(vec![kf(0, 0.2, 0.5), kf(2000, 0.8, 0.5)]);
    let ass = build_ass(&[cue(0, 4000, "hi")], (0, 4000), 0, &look, None, 100).unwrap();
    let e = events(&ass);
    assert_eq!(e.len(), 2, "one event for each stretch between keyframes");
    // 0.2 of 512 is 102.4, 0.8 of 512 is 409.6, and half the height is 144
    assert!(e[0].starts_with("Dialogue: 0,0:00:00.00,0:00:02.00,"));
    assert!(
        e[0].contains("{\\an5\\move(102.4,144.0,409.6,144.0,0,2000)}hi"),
        "{}",
        e[0]
    );
    assert!(e[1].starts_with("Dialogue: 0,0:00:02.00,0:00:04.00,"));
    assert!(e[1].contains("{\\an5\\pos(409.6,144.0)}hi"), "{}", e[1]);
}

#[test]
fn a_line_that_ends_between_keyframes_stops_part_way_along_the_line() {
    // the path goes from the left edge to the right over 4 s; the line is on for 1 s of it
    let look = pathed(vec![kf(0, 0.0, 0.5), kf(4000, 1.0, 0.5)]);
    let ass = build_ass(&[cue(1000, 2000, "hi")], (0, 4000), 0, &look, None, 100).unwrap();
    let e = events(&ass);
    assert_eq!(e.len(), 1);
    // a quarter and then half way across: 128 and 256
    assert!(
        e[0].contains("\\move(128.0,144.0,256.0,144.0,0,1000)"),
        "{}",
        e[0]
    );
}

#[test]
fn keyframe_times_are_the_clips_own_not_the_media_s() {
    // the clip starts 10 s into the film; the keyframe at 2 s of the clip is at 12 s of it
    let look = pathed(vec![kf(0, 0.0, 0.5), kf(2000, 1.0, 0.5)]);
    let ass = build_ass(
        &[cue(10_000, 14_000, "hi")],
        (10_000, 14_000),
        0,
        &look,
        None,
        100,
    )
    .unwrap();
    let e = events(&ass);
    assert_eq!(e.len(), 2);
    assert!(
        e[0].contains("0:00:00.00,0:00:02.00")
            && e[0].contains("\\move(0.0,144.0,512.0,144.0,0,2000)")
    );
    assert!(e[1].contains("0:00:02.00,0:00:04.00") && e[1].contains("\\pos(512.0,144.0)"));
}

#[test]
fn several_lines_each_follow_the_same_path() {
    let look = pathed(vec![kf(0, 0.0, 0.5), kf(3000, 1.0, 0.5)]);
    let cues = [cue(0, 1000, "a"), cue(1500, 2500, "b")];
    let ass = build_ass(&cues, (0, 3000), 0, &look, None, 100).unwrap();
    let e = events(&ass);
    assert_eq!(
        e.len(),
        2,
        "keyframes outside a line's own time do not split it"
    );
    assert!(
        e[0].contains("\\move(0.0,144.0,170.7,144.0,0,1000)"),
        "{}",
        e[0]
    );
    assert!(
        e[1].contains("\\move(256.0,144.0,426.7,144.0,0,1000)"),
        "{}",
        e[1]
    );
}

#[test]
fn a_path_places_the_text_even_without_x_and_y_and_keeps_the_alignment() {
    let left = TextLook {
        align: TextAlign::Left,
        path: vec![kf(0, 0.5, 0.25)],
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &left, None, 100).unwrap();
    // a single keyframe is a place to stay; centred on it vertically, left-aligned
    assert!(events(&ass)[0].contains("{\\an4\\pos(256.0,72.0)}hi"));
}

#[test]
fn words_that_appear_in_a_later_stretch_are_timed_from_that_stretch() {
    let mut look = pathed(vec![kf(0, 0.0, 0.5), kf(2000, 1.0, 0.5)]);
    look.reveal = TextReveal::Word;
    let ass = build_ass(
        &[cue(0, 4000, "aaaa bbbb cccc dddd")],
        (0, 4000),
        0,
        &look,
        None,
        100,
    )
    .unwrap();
    let e = events(&ass);
    assert_eq!(e.len(), 2);
    // said at 0, 800, 1600 and 2400 ms. In the first stretch (0 to 2 s) the last is still
    // hidden when the stretch ends; in the second, three are already up, the last at 400.
    assert_eq!(reveal_times(e[0]), vec![800, 1600, 2400]);
    assert_eq!(
        reveal_times(e[1]),
        vec![400],
        "2400 - 2000, from the second stretch's start"
    );
    assert!(e[1].contains("}aaaa") && e[1].matches("\\alpha&H00&\\fscx100\\fscy100").count() == 3);
}

#[test]
fn a_slide_becomes_a_fade_while_the_text_is_moving() {
    let mut look = pathed(vec![kf(0, 0.1, 0.5), kf(2000, 0.9, 0.5)]);
    look.anim = TextAnim::SlideUp;
    let ass = build_ass(&[cue(500, 3500, "hi")], (0, 4000), 0, &look, None, 100).unwrap();
    let e = events(&ass);
    assert!(e[0].contains("\\fad(180,0)"), "{}", e[0]);
    // the only \move is the path's, none is a 180 ms slide
    assert!(e.iter().all(|l| !l.contains(",0,180)")), "{e:?}");
    // the fade is on the first stretch only
    assert!(e.iter().skip(1).all(|l| !l.contains("\\fad(")));
}

#[test]
fn a_path_with_no_usable_keyframes_leaves_the_text_where_it_was_placed() {
    let look = TextLook {
        x: Some(0.25),
        y: Some(0.5),
        path: vec![kf(0, f32::NAN, 0.5)],
        ..Default::default()
    };
    let ass = build_ass(&[cue(0, 1000, "hi")], (0, 1000), 0, &look, None, 100).unwrap();
    assert_eq!(events(&ass).len(), 1);
    assert!(events(&ass)[0].contains("{\\an5\\pos(128.0,144.0)}hi"));
}

#[test]
fn times_are_written_in_centiseconds() {
    assert_eq!(ass_time(0), "0:00:00.00");
    assert_eq!(ass_time(61_234), "0:01:01.23");
    assert_eq!(ass_time(3_600_000), "1:00:00.00");
    assert_eq!(ass_time(-5), "0:00:00.00");
}
