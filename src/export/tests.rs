use super::*;

#[test]
fn gif_filter_appends_palette() {
    let vf = video_filter(Format::Gif, "fps=15,scale=480:-2:flags=lanczos");
    assert!(vf.contains("palettegen"));
    assert!(vf.contains("paletteuse"));
}

#[test]
fn non_gif_filter_is_core() {
    let core = "fps=15,scale=480:-2:flags=lanczos";
    assert_eq!(video_filter(Format::Mp4, core), core);
}

#[test]
fn source_caption_adds_subtitles_filter() {
    let core = core_chain(
        480,
        15,
        Some(&Caption::source(None)),
        "/app/fonts/DejaVuSans.ttf",
        Some("/data/tmp/abc.srt"),
        None,
        None,
        270,
    );
    assert!(core.contains("subtitles="));
    assert!(core.contains("abc.srt"));
}

#[test]
fn source_caption_with_override_cues_replaces_indexed_lines() {
    let dir = tempfile::tempdir().unwrap();
    let indexed = vec![Cue {
        start_ms: 0,
        end_ms: 1000,
        text: "original indexed line".into(),
    }];
    let edited = vec![Cue {
        start_ms: 0,
        end_ms: 1000,
        text: "corrected and shortened".into(),
    }];
    let caption = Caption::source(Some(edited));
    let (_core, files) = prepare_captions(
        dir.path(),
        "job",
        480,
        15,
        Some(&caption),
        &indexed,
        0,
        1000,
        0,
        "/app/fonts/DejaVuSans.ttf",
        270,
    )
    .unwrap();
    let srt = std::fs::read_to_string(files.srt.as_ref().unwrap()).unwrap();
    assert!(srt.contains("corrected and shortened"));
    assert!(!srt.contains("original indexed line"));
}

#[test]
fn source_caption_without_override_falls_back_to_passed_cues() {
    let dir = tempfile::tempdir().unwrap();
    let indexed = vec![Cue {
        start_ms: 0,
        end_ms: 1000,
        text: "untouched indexed line".into(),
    }];
    let caption = Caption::source(None);
    let (_core, files) = prepare_captions(
        dir.path(),
        "job2",
        480,
        15,
        Some(&caption),
        &indexed,
        0,
        1000,
        0,
        "/app/fonts/DejaVuSans.ttf",
        270,
    )
    .unwrap();
    let srt = std::fs::read_to_string(files.srt.as_ref().unwrap()).unwrap();
    assert!(srt.contains("untouched indexed line"));
}

#[test]
fn meme_caption_adds_two_drawtext() {
    let cap = Caption::meme(Some("TOP".into()), Some("BOT".into()), 40);
    let core = core_chain(
        480,
        15,
        Some(&cap),
        "/app/fonts/Impact.ttf",
        None,
        Some("/data/tmp/t.txt"),
        Some("/data/tmp/b.txt"),
        270,
    );
    assert_eq!(core.matches("drawtext=").count(), 2);
    assert!(core.contains("fontsize=40"));
}

#[test]
fn export_args_seeks_before_input() {
    let args = export_args(
        "/media/x.mkv",
        1000,
        4000,
        "fps=15",
        &codec_args(Format::Mp4, 80, AudioMode::None),
        None,
        "/out/x.mp4",
    );
    let ss = args.iter().position(|a| a == "-ss").unwrap();
    let i = args.iter().position(|a| a == "-i").unwrap();
    assert!(ss < i);
    assert!(args.contains(&"3.000".to_string())); // 3s duration
}

#[test]
fn filename_is_sanitized_with_extension() {
    assert_eq!(
        safe_filename(Some("../../etc/passwd"), Format::Gif, "clip"),
        "passwd.gif"
    );
    assert_eq!(
        safe_filename(Some("My Clip!"), Format::Webp, "clip"),
        "My Clip_.webp"
    );
    assert_eq!(safe_filename(None, Format::Mp4, "clip"), "clip.mp4");
    assert_eq!(
        safe_filename(Some("  "), Format::Webm, "fallback"),
        "fallback.webm"
    );
}

#[test]
fn quality_maps_to_sane_crf() {
    assert!(x264_crf(100) < x264_crf(1));
    assert!(vp9_crf(100) < vp9_crf(1));
    assert!((18..=20).contains(&x264_crf(100)));
}

#[test]
fn validate_audio_format_rejects_gif_and_webp() {
    assert!(validate_audio_format(Format::Gif, true).is_some());
    assert!(validate_audio_format(Format::Webp, true).is_some());
    assert!(validate_audio_format(Format::Mp4, true).is_none());
    assert!(validate_audio_format(Format::Webm, true).is_none());
    // audio: false is always fine, regardless of format.
    assert!(validate_audio_format(Format::Gif, false).is_none());
}

#[test]
fn codec_args_audio_modes_for_mp4() {
    let none = codec_args(Format::Mp4, 80, AudioMode::None);
    assert!(none.contains(&"-an".to_string()));

    let encode = codec_args(Format::Mp4, 80, AudioMode::Encode);
    assert!(encode.windows(2).any(|w| w == ["-c:a", "libopus"]));
    assert!(!encode.contains(&"-an".to_string()));

    let copy = codec_args(Format::Mp4, 80, AudioMode::Copy);
    assert!(copy.windows(2).any(|w| w == ["-c:a", "copy"]));
}

#[test]
fn codec_args_gif_ignores_audio_mode() {
    // GIF has no audio channel at all -- Encode/Copy are simply ignored,
    // never asserted, since this function has no request context to
    // error out with (validate_audio_format is the real guard).
    assert_eq!(
        codec_args(Format::Gif, 80, AudioMode::Encode),
        codec_args(Format::Gif, 80, AudioMode::None)
    );
}

#[test]
fn sequence_clip_audio_args_map_real_audio_when_present() {
    let args = sequence_clip_args_with_audio(
        Path::new("/media/x.mkv"),
        0,
        3000,
        "fps=15",
        true,
        Path::new("/tmp/out.mp4"),
    );
    assert!(args.windows(2).any(|w| w == ["-map", "0:a:0"]));
    assert!(!args
        .iter()
        .any(|a| a == "anullsrc=channel_layout=stereo:sample_rate=48000"));
}

#[test]
fn sequence_clip_audio_args_substitute_silence_when_absent() {
    let args = sequence_clip_args_with_audio(
        Path::new("/media/x.mkv"),
        0,
        3000,
        "fps=15",
        false,
        Path::new("/tmp/out.mp4"),
    );
    assert!(args.windows(2).any(|w| w == ["-map", "1:a:0"]));
    assert!(args
        .iter()
        .any(|a| a.starts_with("anullsrc=channel_layout=stereo")));
    assert!(args.contains(&"-shortest".to_string()));
}

#[test]
fn escapes_colon_in_filter_path() {
    assert_eq!(escape_filter_path("C:/x/y.srt"), "C\\:/x/y.srt");
}

#[test]
fn canvas_dims_are_even_16_9() {
    assert_eq!(canvas_dims(480), (480, 270));
    assert_eq!(canvas_dims(640), (640, 360));
    // Odd width is rounded down to even.
    assert_eq!(canvas_dims(481), (480, 270));
}

#[test]
fn clip_core_chain_scales_and_pads_to_canvas() {
    let core = clip_core_chain(
        480,
        270,
        15,
        None,
        "/app/fonts/DejaVuSans.ttf",
        None,
        None,
        None,
        270,
    );
    assert!(core.contains("scale=480:270:force_original_aspect_ratio=decrease"));
    assert!(core.contains("pad=480:270"));
}

#[test]
fn concat_list_quotes_and_escapes_paths() {
    let list = concat_list(&[
        PathBuf::from("/tmp/a.mp4"),
        PathBuf::from("/tmp/can't stop.mp4"),
    ]);
    assert_eq!(list, "file '/tmp/a.mp4'\nfile '/tmp/can'\\''t stop.mp4'");
}

#[test]
fn validate_sequence_rejects_empty() {
    assert!(validate_sequence(&[]).is_err());
}

#[test]
fn validate_sequence_rejects_backwards_or_overlong_clip() {
    let backwards = vec![SequenceClipSpec {
        rating_key: "a".into(),
        start_ms: 5000,
        end_ms: 1000,
        caption: None,
        subtitle_offset_ms: 0,
    }];
    assert!(validate_sequence(&backwards).is_err());

    let too_long = vec![SequenceClipSpec {
        rating_key: "a".into(),
        start_ms: 0,
        end_ms: MAX_CLIP_MS + 1,
        caption: None,
        subtitle_offset_ms: 0,
    }];
    assert!(validate_sequence(&too_long).is_err());
}

#[test]
fn validate_sequence_rejects_total_over_cap() {
    // Five clips just under the per-clip cap, comfortably over the total cap.
    let clips: Vec<_> = (0..5)
        .map(|i| SequenceClipSpec {
            rating_key: format!("r{i}"),
            start_ms: 0,
            end_ms: MAX_CLIP_MS,
            caption: None,
            subtitle_offset_ms: 0,
        })
        .collect();
    // Five clips at the per-clip cap comfortably exceed the total cap.
    assert!(validate_sequence(&clips).is_err());
}

#[test]
fn validate_sequence_rejects_too_many_clips() {
    let clips: Vec<_> = (0..MAX_SEQUENCE_CLIPS + 1)
        .map(|i| SequenceClipSpec {
            rating_key: format!("r{i}"),
            start_ms: 0,
            end_ms: 100,
            caption: None,
            subtitle_offset_ms: 0,
        })
        .collect();
    assert!(validate_sequence(&clips).is_err());
}

#[test]
fn validate_sequence_accepts_reasonable_sequence() {
    let clips = vec![
        SequenceClipSpec {
            rating_key: "a".into(),
            start_ms: 0,
            end_ms: 3000,
            caption: None,
            subtitle_offset_ms: 0,
        },
        SequenceClipSpec {
            rating_key: "b".into(),
            start_ms: 1000,
            end_ms: 4000,
            caption: None,
            subtitle_offset_ms: 0,
        },
    ];
    assert!(validate_sequence(&clips).is_ok());
}

#[test]
fn clip_core_chain_still_supports_captions() {
    let cap = Caption::meme(Some("TOP".into()), None, 30);
    let core = clip_core_chain(
        480,
        270,
        15,
        Some(&cap),
        "/app/fonts/Impact.ttf",
        None,
        Some("/data/tmp/t.txt"),
        None,
        270,
    );
    assert!(core.contains("drawtext="));
}

fn has_pair(args: &[String], key: &str, value: &str) -> bool {
    args.windows(2).any(|w| w[0] == key && w[1] == value)
}

#[test]
fn exports_never_copy_the_sources_tags() {
    let args = export_args(
        "/media/Some.Movie.2021.2160p.REMUX-GRP.mkv",
        0,
        3000,
        "fps=15",
        &codec_args(Format::Mp4, 80, AudioMode::None),
        None,
        "/out/x.mp4",
    );
    // global, per-stream and chapter metadata: none of it is carried over
    assert!(has_pair(&args, "-map_metadata", "-1"));
    assert!(has_pair(&args, "-map_chapters", "-1"));
    assert!(has_pair(&args, "-map_metadata:s:v", "-1"));
    assert!(has_pair(&args, "-map_metadata:s:a", "-1"));
    // no soft-subtitle or data tracks (captions are burned in)
    assert!(args.contains(&"-sn".to_string()) && args.contains(&"-dn".to_string()));
    // no encoder/muxer version strings
    assert!(has_pair(&args, "-fflags", "+bitexact"));
    assert!(has_pair(&args, "-flags:v", "+bitexact"));
    assert!(has_pair(&args, "-flags:a", "+bitexact"));
    // and no title unless one is given
    assert!(!args.iter().any(|a| a.starts_with("title=")));
    // all of that is an output option: after the input, before the output file
    let input = args.iter().position(|a| a == "-i").unwrap();
    let strip = args.iter().position(|a| a == "-map_metadata").unwrap();
    assert!(input < strip && strip < args.len() - 1);
    assert_eq!(args.last().unwrap(), "/out/x.mp4");
}

#[test]
fn the_only_tag_an_export_may_carry_is_the_source_title() {
    let args = export_args(
        "/media/x.mkv",
        0,
        3000,
        "fps=15",
        &codec_args(Format::Webm, 80, AudioMode::None),
        Some("The Lantern Cove - S02E03 - Tideline"),
        "/out/x.webm",
    );
    let titles: Vec<&String> = args.iter().filter(|a| a.starts_with("title=")).collect();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0], "title=The Lantern Cove - S02E03 - Tideline");
    // every `-metadata` the export sets is that title or an empty `encoder=` (which deletes one)
    for w in args.windows(2) {
        if w[0] == "-metadata" || w[0].starts_with("-metadata:") {
            assert!(
                w[1].starts_with("title=") || w[1] == "encoder=",
                "unexpected tag {}",
                w[1]
            );
        }
    }
}

#[test]
fn titles_are_tidied_before_they_go_into_a_tag() {
    assert_eq!(clean_title("  A\n\tB   C \u{0}D "), "A B C D");
    assert_eq!(clean_title(&"x".repeat(500)).chars().count(), 200);
    // a blank title writes no tag at all
    assert!(!privacy_args(Some("  \n "))
        .iter()
        .any(|a| a.starts_with("title=")));
}

#[test]
fn x264_does_not_stamp_its_settings_into_the_stream() {
    let a = codec_args(Format::Mp4, 80, AudioMode::None);
    assert!(has_pair(&a, "-x264-params", "no-info=1"));
}

#[test]
fn the_source_label_names_the_movie_or_the_show_and_episode() {
    assert_eq!(
        source_label("movie", "Paper Moon Heist", None, None, None, Some(2019)),
        "Paper Moon Heist (2019)"
    );
    assert_eq!(
        source_label("movie", "Untitled", None, None, None, None),
        "Untitled"
    );
    assert_eq!(
        source_label(
            "episode",
            "Tideline",
            Some("The Lantern Cove"),
            Some(2),
            Some(3),
            Some(2018)
        ),
        "The Lantern Cove - S02E03 - Tideline"
    );
    assert_eq!(
        source_label(
            "episode",
            "Tideline",
            Some("The Lantern Cove"),
            None,
            None,
            None
        ),
        "The Lantern Cove - Tideline"
    );
}

#[test]
fn a_sequences_title_lists_each_source_once() {
    let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        combined_source_title(&l(&["A (2019)", "B - S01E01 - X", "A (2019)"])).as_deref(),
        Some("A (2019) / B - S01E01 - X")
    );
    assert_eq!(combined_source_title(&l(&[])), None);
    assert_eq!(combined_source_title(&l(&["", ""])), None);
    let long = combined_source_title(
        &(0..50)
            .map(|i| format!("Source number {i}"))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(long.chars().count() <= 200 && long.ends_with("..."));
}

#[test]
fn credit_source_defaults_to_on() {
    let r: ExportRequest =
        serde_json::from_str(r#"{"rating_key":"1","start_ms":0,"end_ms":1000,"format":"mp4"}"#)
            .unwrap();
    assert!(r.credit_source);
    let r: ExportRequest = serde_json::from_str(
        r#"{"rating_key":"1","start_ms":0,"end_ms":1000,"format":"mp4","credit_source":false}"#,
    )
    .unwrap();
    assert!(!r.credit_source);
}

#[test]
fn a_window_with_no_subtitle_lines_burns_nothing_instead_of_an_empty_srt() {
    let dir = tempfile::tempdir().unwrap();
    let cues = vec![Cue {
        start_ms: 5000,
        end_ms: 6000,
        text: "a line that is outside the clip".into(),
    }];
    let (core, files) = prepare_captions(
        dir.path(),
        "empty-window",
        480,
        15,
        Some(&Caption::source(None)),
        &cues,
        0,
        1000,
        0,
        "/app/fonts/DejaVuSans.ttf",
        270,
    )
    .unwrap();
    assert!(files.srt.is_none());
    assert!(!core.contains("subtitles="), "{core}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    // every line deleted in the editor is the same thing
    let (core, files) = prepare_captions(
        dir.path(),
        "all-deleted",
        480,
        15,
        Some(&Caption::source(Some(vec![]))),
        &cues,
        0,
        1000,
        0,
        "/app/fonts/DejaVuSans.ttf",
        270,
    )
    .unwrap();
    assert!(files.srt.is_none() && !core.contains("subtitles="));
}

#[test]
fn caption_files_are_removed_when_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let cues = vec![Cue {
        start_ms: 0,
        end_ms: 1000,
        text: "hello".into(),
    }];
    let (_core, files) = prepare_captions(
        dir.path(),
        "dropped",
        480,
        15,
        Some(&Caption::source(None)),
        &cues,
        0,
        1000,
        0,
        "/app/fonts/DejaVuSans.ttf",
        270,
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    drop(files);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

const TEST_FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/MemeTestSans.ttf"
);

#[test]
fn the_default_subtitle_style_is_unchanged() {
    // no font chosen, normal size: exactly what it was before fonts could be chosen
    assert_eq!(
        subtitle_style(&Caption::source(None), "/app/fonts/DejaVuSans.ttf"),
        "Outline=2,Shadow=0,MarginV=10"
    );
}

#[test]
fn a_chosen_font_is_named_by_family_and_its_folder_is_searched() {
    let cap = Caption::Source {
        cues: None,
        font: Some("MemeTestSans.ttf".into()),
        font_scale: 100,
    };
    assert_eq!(
        subtitle_style(&cap, TEST_FONT),
        "FontName=Meme Test Sans,Outline=2,Shadow=0,MarginV=10"
    );
    let chain = core_chain(
        480,
        15,
        Some(&cap),
        TEST_FONT,
        Some("/data/tmp/a.srt"),
        None,
        None,
        270,
    );
    assert!(chain.contains("FontName=Meme Test Sans"), "{chain}");
    assert!(
        chain.contains("fontsdir='") && chain.contains("fixtures"),
        "{chain}"
    );
    // a font that can't be read leaves the default font, rather than a broken name
    assert_eq!(
        subtitle_style(&cap, "/nonexistent/Gone.ttf"),
        "Outline=2,Shadow=0,MarginV=10"
    );
}

#[test]
fn text_size_scales_subtitles_and_meme_text() {
    let big = Caption::Source {
        cues: None,
        font: None,
        font_scale: 150,
    };
    assert!(
        subtitle_style(&big, "x.ttf").starts_with("FontSize=24,"),
        "{}",
        subtitle_style(&big, "x.ttf")
    );
    let small = Caption::Source {
        cues: None,
        font: None,
        font_scale: 50,
    };
    assert!(subtitle_style(&small, "x.ttf").starts_with("FontSize=8,"));

    let meme = |scale| Caption::Meme {
        top: Some("T".into()),
        bottom: None,
        font_size: 36,
        font: None,
        font_scale: scale,
    };
    let chain =
        |c: &Caption| core_chain(480, 15, Some(c), "/f.ttf", None, Some("/t.txt"), None, 270);
    assert!(chain(&meme(100)).contains("fontsize=36"));
    assert!(chain(&meme(150)).contains("fontsize=54"));
    assert!(chain(&meme(50)).contains("fontsize=18"));
}

#[test]
fn text_size_is_kept_within_limits() {
    assert_eq!(Caption::source(None).font_scale_pct(), 100);
    let wild = |s| Caption::Source {
        cues: None,
        font: None,
        font_scale: s,
    };
    assert_eq!(wild(0).font_scale_pct(), MIN_FONT_SCALE);
    assert_eq!(wild(100_000).font_scale_pct(), MAX_FONT_SCALE);
    assert_eq!(Caption::None.font_scale_pct(), 100);
}

#[test]
fn a_caption_request_without_font_fields_still_parses_with_the_defaults() {
    let c: Caption = serde_json::from_str(r#"{"mode":"source"}"#).unwrap();
    assert_eq!((c.font_id(), c.font_scale_pct()), (None, 100));
    let c: Caption = serde_json::from_str(r#"{"mode":"meme","top":"hi"}"#).unwrap();
    assert_eq!((c.font_id(), c.font_scale_pct()), (None, 100));
    let c: Caption =
        serde_json::from_str(r#"{"mode":"meme","top":"hi","font":"A.ttf","font_scale":150}"#)
            .unwrap();
    assert_eq!((c.font_id(), c.font_scale_pct()), (Some("A.ttf"), 150));
    // a blank font name means "no font chosen"
    let c: Caption = serde_json::from_str(r#"{"mode":"source","font":"  "}"#).unwrap();
    assert_eq!(c.font_id(), None);
}
