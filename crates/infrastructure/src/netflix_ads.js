// browsingUnicorns, Netflix's ads plan: an ad break plays muted and sped up,
// the ad shown on a paused video is closed. Netflix streams its ads from its
// own servers with the film, so the requests cannot be blocked. Media events
// don't bubble but are caught on the document: nothing runs while no video
// plays.
(() => {
  if (window.__buNetflixAds) return;
  window.__buNetflixAds = true;

  // The ad break's countdown (`25` or `1:05`): Netflix's generated class, as
  // Streaming Enhanced reads it.
  const COUNTDOWN = 'span[class*="mmvz9h"]';
  // The paused video's ad and the button that closes it.
  const PAUSE_AD = 'div[data-uia="pause-ad-title-display"]';
  const PAUSE_AD_CLOSE = 'button[data-uia="pause-ad-expand-button"]';
  // Faster stalls Edge's engine (WebView2), says Streaming Enhanced.
  const RATE = /\bEdg\//.test(navigator.userAgent) ? 3 : 8;
  // Under this many seconds left, the break plays at the normal speed: at
  // RATE, a check (about 4 a second) comes after up to 2 s of video and the
  // film must not start sped up.
  const SLOW_DOWN_UNDER = 3;

  // The seconds the countdown shows; 0 when there's none.
  const secondsLeft = () => {
    const text = document.querySelector(COUNTDOWN)?.textContent?.trim() ?? "";
    const m = /^(?:(\d+):)?(\d+)$/.exec(text);
    return m ? Number(m[1] ?? 0) * 60 + Number(m[2]) : 0;
  };

  // Each video's own speed and sound, kept through a break.
  const saved = new WeakMap();

  const onTime = (event) => {
    const video = event.target;
    if (!(video instanceof HTMLVideoElement)) return;
    const left = secondsLeft();
    if (left > 0) {
      if (!saved.has(video)) {
        saved.set(video, { rate: video.playbackRate, muted: video.muted });
      }
      video.muted = true;
      const rate = left >= SLOW_DOWN_UNDER ? RATE : saved.get(video).rate;
      if (video.playbackRate !== rate) video.playbackRate = rate;
    } else if (saved.has(video)) {
      const { rate, muted } = saved.get(video);
      saved.delete(video);
      video.playbackRate = rate;
      video.muted = muted;
    }
  };

  // The pause ad comes a few seconds after the pause: looked for while the
  // video stays paused, closed once, the video kept paused.
  let watching = 0;
  const onPause = (event) => {
    const video = event.target;
    if (!(video instanceof HTMLVideoElement)) return;
    clearInterval(watching);
    let tries = 0;
    watching = setInterval(() => {
      if (!video.paused || ++tries > 120) {
        clearInterval(watching);
        return;
      }
      const ad = document.querySelector(PAUSE_AD);
      const close = document.querySelector(PAUSE_AD_CLOSE);
      if (!close || !ad?.checkVisibility?.({ opacityProperty: true })) return;
      clearInterval(watching);
      close.click();
      setTimeout(() => video.paused || video.pause(), 100);
    }, 500);
  };

  document.addEventListener("timeupdate", onTime, true);
  document.addEventListener("pause", onPause, true);
})();
