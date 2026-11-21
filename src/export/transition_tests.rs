use super::*;

fn t(kind: TransitionKind, ms: i64) -> Option<Transition> {
    Some(Transition { kind, ms })
}

#[test]
fn every_kind_has_a_name_xfade_knows() {
    let all = [
        (TransitionKind::Fade, "fade"),
        (TransitionKind::FadeBlack, "fadeblack"),
        (TransitionKind::FadeWhite, "fadewhite"),
        (TransitionKind::WipeLeft, "wipeleft"),
        (TransitionKind::WipeRight, "wiperight"),
        (TransitionKind::WipeUp, "wipeup"),
        (TransitionKind::WipeDown, "wipedown"),
        (TransitionKind::SlideLeft, "slideleft"),
        (TransitionKind::SlideRight, "slideright"),
        (TransitionKind::SlideUp, "slideup"),
        (TransitionKind::SlideDown, "slidedown"),
        (TransitionKind::CircleOpen, "circleopen"),
        (TransitionKind::CircleClose, "circleclose"),
        (TransitionKind::Radial, "radial"),
        (TransitionKind::Pixelize, "pixelize"),
        (TransitionKind::Dissolve, "dissolve"),
    ];
    for (kind, name) in all {
        assert_eq!(kind.xfade(), name);
    }
}

#[test]
fn a_transition_is_read_from_its_names() {
    let tr: Transition = serde_json::from_str(r#"{"kind":"wipe_left","ms":500}"#).unwrap();
    assert_eq!(
        tr,
        Transition {
            kind: TransitionKind::WipeLeft,
            ms: 500
        }
    );
    assert!(serde_json::from_str::<Transition>(r#"{"kind":"spin","ms":500}"#).is_err());
}

#[test]
fn transitions_are_checked() {
    let fade = |ms| t(TransitionKind::Fade, ms);
    assert!(validate_transitions(&[(None, 3000), (fade(500), 3000)]).is_none());
    assert!(validate_transitions(&[(None, 3000), (None, 3000)]).is_none());
    assert!(validate_transitions(&[]).is_none());
    // the first clip has nothing to come in over
    assert!(validate_transitions(&[(fade(500), 3000)])
        .unwrap()
        .contains("first clip"));
    // length limits, naming the clip
    assert!(validate_transitions(&[(None, 3000), (fade(50), 3000)])
        .unwrap()
        .contains("#2"));
    assert!(validate_transitions(&[(None, 9000), (fade(4000), 9000)]).is_some());
    // a clip needs room for the transition in and the one out
    assert!(
        validate_transitions(&[(None, 3000), (fade(1000), 1500), (fade(1000), 3000)])
            .unwrap()
            .contains("#2")
    );
    assert!(
        validate_transitions(&[(None, 3000), (fade(1000), 2000), (fade(1000), 3000)]).is_none()
    );
}

#[test]
fn where_each_clip_starts_with_the_overlaps_taken_out() {
    let lens = [3000, 3000, 3000];
    let none = [None, None, None];
    assert_eq!(clip_starts(&lens, &none), vec![0, 3000, 6000]);
    assert_eq!(joined_length(&lens, &none), 9000);
    let tr = [
        None,
        t(TransitionKind::Fade, 1000),
        t(TransitionKind::Fade, 500),
    ];
    assert_eq!(clip_starts(&lens, &tr), vec![0, 2000, 4500]);
    assert_eq!(joined_length(&lens, &tr), 7500);
    // a transition on the first clip is ignored, and so is a short list of them
    assert_eq!(
        clip_starts(&lens, &[t(TransitionKind::Fade, 1000)]),
        vec![0, 3000, 6000]
    );
    assert_eq!(joined_length(&[], &[]), 0);
    assert!(!has_transitions(&none));
    assert!(!has_transitions(&[t(TransitionKind::Fade, 1000), None]));
    assert!(has_transitions(&tr));
}

#[test]
fn the_join_blends_a_transition_and_cuts_the_rest() {
    let lens = [3000, 3000, 3000];
    let tr = [None, t(TransitionKind::WipeLeft, 1000), None];
    let g = join_graph(&lens, &tr, 15, true);
    // every part is made the same before it is joined
    assert!(
        g.contains("[0:v]fps=15,settb=AVTB,setpts=PTS-STARTPTS,setsar=1,format=yuv420p[nv0]"),
        "{g}"
    );
    assert!(g.contains("[2:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS[na2]"), "{g}");
    // the blend starts one transition before the first clip ends, and the sound crosses with it
    assert!(
        g.contains("[nv0][nv1]xfade=transition=wipeleft:duration=1.000:offset=2.000[v1]"),
        "{g}"
    );
    assert!(
        g.contains("[na0][na1]acrossfade=d=1.000:c1=tri:c2=tri[a1]"),
        "{g}"
    );
    // the cut after it is a concatenation, and its result is the sequence
    assert!(
        g.contains("[v1][a1][nv2][na2]concat=n=2:v=1:a=1[vout][aout]"),
        "{g}"
    );
}

#[test]
fn later_transitions_start_where_the_joined_part_so_far_ends() {
    let lens = [3000, 3000, 3000];
    let tr = [
        None,
        t(TransitionKind::Fade, 1000),
        t(TransitionKind::Dissolve, 500),
    ];
    let g = join_graph(&lens, &tr, 10, false);
    // 3 s + 3 s - 1 s = 5 s so far: the next one starts at 4.5
    assert!(
        g.contains("[nv0][nv1]xfade=transition=fade:duration=1.000:offset=2.000[v1]"),
        "{g}"
    );
    assert!(
        g.contains("[v1][nv2]xfade=transition=dissolve:duration=0.500:offset=4.500[vout]"),
        "{g}"
    );
    assert!(
        !g.contains("acrossfade") && !g.contains("[aout]"),
        "no sound, none made: {g}"
    );
}

#[test]
fn a_single_part_just_passes_through() {
    let g = join_graph(&[3000], &[None], 10, true);
    assert!(
        g.contains("[nv0]null[vout]") && g.contains("[na0]anull[aout]"),
        "{g}"
    );
}

#[test]
fn the_join_command_maps_the_ends_and_keeps_pcm_sound() {
    let parts = vec![PathBuf::from("a.mov"), PathBuf::from("b.mov")];
    let tr = [None, t(TransitionKind::Fade, 500)];
    let args = join_args(&parts, &[3000, 3000], &tr, 15, true, Path::new("out.mov"));
    assert_eq!(args.iter().filter(|a| *a == "-i").count(), 2);
    let at = |s: &str| args.iter().position(|a| a == s).unwrap();
    assert_eq!(
        args[at("-filter_complex") + 1],
        join_graph(&[3000, 3000], &tr, 15, true)
    );
    assert!(args.windows(2).any(|w| w == ["-map", "[vout]"]));
    assert!(args.windows(2).any(|w| w == ["-map", "[aout]"]));
    assert!(args.windows(2).any(|w| w == ["-c:a", "pcm_s16le"]));
    assert_eq!(args.last().unwrap(), "out.mov");
    // without sound: no audio mapped, and none written
    let silent = join_args(&parts, &[3000, 3000], &tr, 15, false, Path::new("out.mp4"));
    assert!(!silent.iter().any(|a| a == "[aout]"));
    assert!(silent.iter().any(|a| a == "-an"));
}
