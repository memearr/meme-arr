use super::*;

fn song() -> Soundtrack {
    Soundtrack {
        id: "abc-123".into(),
        start_ms: 30_000,
        at_ms: 0,
        volume: 0.5,
        fade_in_ms: 0,
        fade_out_ms: 0,
        duck: 0.0,
        file: Some(PathBuf::from("/data/music/abc-123.mp3")),
    }
}

fn check(m: Option<&Soundtrack>, vol: f32, fmt: Format, audio: bool) -> Option<String> {
    validate_music(m, vol, 5000, fmt, audio)
}

#[test]
fn nothing_to_say_about_no_music_and_the_clip_as_it_is() {
    assert_eq!(check(None, 1.0, Format::Gif, false), None);
    assert_eq!(check(None, 1.5, Format::Mp4, true), None);
}

#[test]
fn volumes_are_between_silent_and_twice_as_loud() {
    assert!(check(None, 2.5, Format::Mp4, true).is_some());
    assert!(check(None, -0.1, Format::Mp4, true).is_some());
    assert!(check(None, f32::NAN, Format::Mp4, true).is_some());
    let mut m = song();
    assert_eq!(check(Some(&m), 1.0, Format::Mp4, false), None);
    m.volume = 2.01;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
    m.volume = 0.0;
    assert_eq!(check(Some(&m), 1.0, Format::Mp4, false), None);
}

#[test]
fn a_clip_volume_needs_the_clips_sound_kept() {
    let why = check(None, 0.5, Format::Mp4, false).unwrap();
    assert!(why.contains("sound is kept"), "{why}");
}

#[test]
fn only_formats_with_sound_take_music() {
    for fmt in [Format::Gif, Format::Webp] {
        let why = check(Some(&song()), 1.0, fmt, false).unwrap();
        assert!(why.contains("MP4 or WebM"), "{why}");
    }
    assert_eq!(check(Some(&song()), 1.0, Format::Webm, false), None);
}

#[test]
fn timing_stays_inside_the_clip_and_the_song() {
    let mut m = song();
    m.at_ms = 5000;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
    m.at_ms = 4999;
    assert_eq!(check(Some(&m), 1.0, Format::Mp4, false), None);
    m.at_ms = -1;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
    let mut m = song();
    m.start_ms = -5;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
    m.start_ms = MAX_SONG_START_MS + 1;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
    let mut m = song();
    m.at_ms = 3000; // 2 s of music
    m.fade_in_ms = 2000;
    assert_eq!(check(Some(&m), 1.0, Format::Mp4, false), None);
    m.fade_out_ms = 2001;
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
}

#[test]
fn a_song_id_is_checked() {
    let mut m = song();
    m.id = "../x".into();
    assert!(check(Some(&m), 1.0, Format::Mp4, false).is_some());
}

#[test]
fn no_music_and_no_volume_change_leaves_the_sound_alone() {
    assert_eq!(plan_audio(None, 1.0, true, 5000, 1), None);
    // a song whose file is not known yet is not mixed
    let mut m = song();
    m.file = None;
    assert_eq!(plan_audio(Some(&m), 1.0, false, 5000, 1), None);
    // a volume change with no sound to change
    assert_eq!(plan_audio(None, 0.5, false, 5000, 1), None);
}

#[test]
fn the_clips_own_volume_alone_is_a_gain_and_a_limiter() {
    let mix = plan_audio(None, 0.5, true, 5000, 1).unwrap();
    assert!(mix.input_args.is_empty());
    assert!(mix
        .graph
        .starts_with("[0:a:0]aformat=channel_layouts=stereo,volume=0.500[ca];[ca]"));
    assert!(mix.graph.ends_with("alimiter=limit=0.95[aout]"));
}

#[test]
fn music_alone_starts_at_the_chosen_point_and_is_cut_to_the_clip() {
    let mix = plan_audio(Some(&song()), 1.0, false, 5000, 1).unwrap();
    assert_eq!(
        mix.input_args,
        [
            "-ss",
            "30.000",
            "-t",
            "5.000",
            "-i",
            "/data/music/abc-123.mp3"
        ]
    );
    assert!(mix
        .graph
        .contains("[1:a:0]aresample=48000,aformat=channel_layouts=stereo,volume=0.500[mu]"));
    assert!(mix
        .graph
        .contains("[mu]atrim=end=5.000,alimiter=limit=0.95[aout]"));
    assert!(!mix.graph.contains("amix"));
    assert!(!mix.graph.contains("afade") && !mix.graph.contains("adelay"));
}

#[test]
fn music_and_the_clips_sound_are_mixed_without_the_mixer_turning_them_down() {
    let mix = plan_audio(Some(&song()), 0.8, true, 5000, 3).unwrap();
    assert!(mix
        .graph
        .contains("[0:a:0]aformat=channel_layouts=stereo,volume=0.800[ca]"));
    assert!(mix.graph.contains("[3:a:0]"));
    assert!(mix
        .graph
        .contains("[ca][mu]amix=inputs=2:duration=longest:normalize=0,atrim=end=5.000"));
}

#[test]
fn ducking_makes_the_clips_sound_the_songs_sidechain() {
    let mut m = song();
    m.duck = 1.0;
    let mix = plan_audio(Some(&m), 1.0, true, 5000, 1).unwrap();
    assert!(
        mix.graph.contains("[ca0]asplit=2[ca][cs0]"),
        "{}",
        mix.graph
    );
    assert!(mix
        .graph
        .contains("[mu0][sc]sidechaincompress=threshold=0.02:ratio=20.0"));
    assert!(
        mix.graph.contains("[ca][mu]amix"),
        "the mix still takes the clip and the ducked song"
    );
    // no ducking: the plain graph
    let plain = plan_audio(Some(&song()), 1.0, true, 5000, 1).unwrap();
    assert!(!plain.graph.contains("sidechain") && !plain.graph.contains("asplit"));
    // a clip without sound has nothing to duck for
    let silent = plan_audio(Some(&m), 1.0, false, 5000, 1).unwrap();
    assert!(!silent.graph.contains("sidechain"));
}

#[test]
fn ducking_is_checked() {
    let mut m = song();
    m.duck = 0.5;
    assert_eq!(check(Some(&m), 1.0, Format::Mp4, true), None);
    assert!(check(Some(&m), 1.0, Format::Mp4, false)
        .unwrap()
        .contains("ducking"));
    m.duck = 1.5;
    assert!(check(Some(&m), 1.0, Format::Mp4, true)
        .unwrap()
        .contains("between 0 and 1"));
    m.duck = f32::NAN;
    assert!(check(Some(&m), 1.0, Format::Mp4, true).is_some());
}

#[test]
fn fades_are_in_the_songs_own_time_and_the_delay_comes_after_them() {
    let mut m = song();
    m.at_ms = 1000;
    m.fade_in_ms = 500;
    m.fade_out_ms = 1500;
    let mix = plan_audio(Some(&m), 1.0, false, 5000, 1).unwrap();
    // 4 s of music (5 s clip, coming in at 1 s)
    assert_eq!(mix.input_args[3], "4.000");
    let g = &mix.graph;
    let (fi, fo, ad) = (
        g.find("afade=t=in:st=0:d=0.500").unwrap(),
        g.find("afade=t=out:st=2.500:d=1.500").unwrap(),
        g.find("adelay=1000:all=1").unwrap(),
    );
    assert!(fi < fo && fo < ad, "{g}");
}

fn flags(args: &[String], flag: &str) -> Vec<usize> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == flag)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn a_plain_export_gets_the_song_as_a_second_input_and_maps_the_mix() {
    let args = export_args(
        "clip.mkv",
        1000,
        6000,
        "scale=480:-2",
        &["-c:v".into(), "libx264".into()],
        None,
        "out.mp4",
    );
    let mix = plan_audio(Some(&song()), 1.0, false, 5000, 1).unwrap();
    let got = splice_audio(args, &mix);
    // two inputs, the clip's first
    let inputs = flags(&got, "-i");
    assert_eq!(inputs.len(), 2);
    assert_eq!(got[inputs[0] + 1], "clip.mkv");
    assert_eq!(got[inputs[1] + 1], "/data/music/abc-123.mp3");
    // the song's own seek and length sit just before its -i
    assert_eq!(got[inputs[1] - 4], "-ss");
    // the picture's filter stays, the sound graph is added, and the output takes both
    let fc = flags(&got, "-filter_complex")[0];
    assert_eq!(got[fc + 1], mix.graph);
    assert!(got.windows(2).any(|w| w == ["-vf", "scale=480:-2"]));
    assert!(got
        .windows(4)
        .any(|w| w == ["-map", "0:v:0", "-map", "[aout]"]));
    assert_eq!(got.last().unwrap(), "out.mp4");
    // every input comes before the filters
    assert!(inputs.iter().all(|i| *i < fc));
}

fn layered() -> Vec<String> {
    let plan = LayerPlan {
        input_args: vec![
            "-loop".into(),
            "1".into(),
            "-t".into(),
            "5.000".into(),
            "-i".into(),
            "dot.png".into(),
        ],
        graph: "[0:v]null[b0];[b0][1:v]overlay[out]".into(),
    };
    export_args_with_layers(
        "clip.mkv",
        0,
        5000,
        &plan,
        true,
        &["-c:v".into(), "libx264".into()],
        None,
        "out.mp4",
    )
}

#[test]
fn a_layered_export_gets_the_song_after_the_layers_and_the_mix_replaces_the_clips_sound() {
    let mix = plan_audio(Some(&song()), 1.0, true, 5000, 2).unwrap();
    let got = splice_audio(layered(), &mix);
    let inputs = flags(&got, "-i");
    assert_eq!(inputs.len(), 3);
    assert_eq!(got[inputs[1] + 1], "dot.png");
    assert_eq!(got[inputs[2] + 1], "/data/music/abc-123.mp3");
    let fc = flags(&got, "-filter_complex")[0];
    assert!(got[fc + 1].starts_with("[0:v]null[b0];[b0][1:v]overlay[out];[0:a:0]"));
    assert!(got[fc + 1].ends_with("[aout]"));
    assert!(
        !got.contains(&"0:a:0?".to_string()),
        "the clip's own sound is no longer mapped"
    );
    assert!(got
        .windows(4)
        .any(|w| w == ["-map", "[out]", "-map", "[aout]"]));
}

#[test]
fn a_layered_export_without_the_clips_sound_still_maps_the_mix() {
    let plan = LayerPlan {
        input_args: vec![],
        graph: "[0:v]null[out]".into(),
    };
    let args = export_args_with_layers("clip.mkv", 0, 5000, &plan, false, &[], None, "out.mp4");
    let mix = plan_audio(Some(&song()), 1.0, false, 5000, 1).unwrap();
    let got = splice_audio(args, &mix);
    assert!(
        got.windows(4)
            .any(|w| w == ["-map", "[out]", "-map", "[aout]"]),
        "{got:?}"
    );
}
