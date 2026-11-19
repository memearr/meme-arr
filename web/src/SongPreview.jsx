import { useEffect, useRef } from "react";
import { musicSrc, previewState } from "./musicModel.js";

/**
 * A song playing in the page alongside a player: on when the player reaches it, at the right
 * place in the song, at the right volume (fades included; the page tops out at full volume).
 * `relMs` is the player's position in the clip the song belongs to, `clipMs` that clip's length,
 * `playing` whether the player is playing, and `hear` whether to make any sound.
 * It draws only the (hidden) audio element.
 */
export function SongPreview({ music, relMs, clipMs, playing, hear = true }) {
  const player = useRef(null);
  const state = previewState(music, relMs, clipMs);
  // in a sequence the playhead goes on past the end of this clip: the song stops there
  const on = state.on && relMs < clipMs;

  useEffect(() => {
    if (player.current) player.current.volume = hear ? state.gain : 0;
  }, [state.gain, hear, music?.id]);

  useEffect(() => {
    const a = player.current;
    if (!a || !music) return;
    if (!playing || !on || !hear) {
      a.pause();
      return;
    }
    const want = state.songMs / 1000;
    if (a.paused || Math.abs(a.currentTime - want) > 0.35) {
      try {
        a.currentTime = want;
      } catch (_) {
        // not loaded yet: it starts from the beginning of what it has
      }
    }
    if (a.paused) a.play().catch(() => {});
  }, [playing, on, state.songMs, hear, music?.id]);

  if (!music) return null;
  return <audio ref={player} src={musicSrc(music.id)} preload="auto" loop={false} />;
}
