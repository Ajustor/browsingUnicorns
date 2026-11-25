// browsingUnicorns, YouTube's sponsored parts: the sponsor reads and
// self-promotions creators put in their videos are skipped, with SponsorBlock's
// segments (sponsor.ajay.app, submitted by its users; CC BY-NC-SA 4.0). The
// video is asked by the first 4 characters of its id's SHA-256, as SponsorBlock
// does, so the server doesn't learn which one is watched. A note over the
// player tells what was skipped, with a button to go back to it. Nothing is
// skipped during an ad.
(() => {
  if (window.__buYoutubeSponsors) return;
  window.__buYoutubeSponsors = true;

  const API = "https://sponsor.ajay.app/api/skipSegments/";
  const CATEGORIES = ["sponsor", "selfpromo"];
  // A segment ending this close to where the video is is left to play out.
  const END_MARGIN = 0.3;
  // Shorter segments are not skipped (a mark, not a part).
  const MIN_LENGTH = 1;
  const fr = /^fr\b/i.test(navigator.language || "");
  const TEXT = {
    sponsor: fr ? "Sponsor passé" : "Sponsor skipped",
    selfpromo: fr ? "Autopromo passée" : "Self-promotion skipped",
    undo: fr ? "Revoir" : "Watch it",
  };

  // The id of the video the address shows, `null` off a video page.
  const videoId = () => {
    const url = new URL(location.href);
    const v = url.searchParams.get("v");
    if (v) return v;
    const m = /^\/(?:shorts|live|embed)\/([\w-]{11})/.exec(url.pathname);
    return m ? m[1] : null;
  };

  const sha256 = async (text) => {
    const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
    return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
  };

  // The video whose segments are known or asked for, and them.
  let current = null;
  let segments = [];
  // The segments skipped once, or gone back to: not skipped again.
  let done = new Set();

  const load = async (id) => {
    current = id;
    segments = [];
    done = new Set();
    try {
      const prefix = (await sha256(id)).slice(0, 4);
      const query =
        "?categories=" + encodeURIComponent(JSON.stringify(CATEGORIES)) +
        "&actionTypes=" + encodeURIComponent(JSON.stringify(["skip"]));
      const response = await fetch(API + prefix + query, { credentials: "omit" });
      if (!response.ok) return; // 404: none for these videos.
      const videos = await response.json();
      const found = (videos.find((v) => v.videoID === id)?.segments ?? [])
        .filter((s) => s.actionType === "skip" && CATEGORIES.includes(s.category))
        .map((s) => ({ start: s.segment[0], end: s.segment[1], category: s.category, id: s.UUID }))
        .filter((s) => s.end - s.start >= MIN_LENGTH)
        .sort((a, b) => a.start - b.start);
      if (current === id) segments = found;
    } catch (e) {
      // Offline, or the server down: the video plays as it is.
    }
  };

  // An ad plays in the player.
  const inAd = (video) => !!video.closest(".ad-showing, .ad-interrupting");

  let note = null;
  let noteTimer = 0;
  const showNote = (video, segment) => {
    const player = video.closest("#movie_player, .html5-video-player") || document.body;
    note?.remove();
    clearTimeout(noteTimer);
    note = document.createElement("div");
    note.style.cssText =
      "position:absolute;left:12px;bottom:64px;z-index:1000;display:flex;align-items:center;" +
      "gap:10px;padding:8px 10px 8px 14px;border-radius:10px;background:rgba(20,16,32,.88);" +
      "color:#fff;font:500 14px/1.2 Roboto,Arial,sans-serif;box-shadow:0 4px 16px rgba(0,0,0,.4)";
    const label = document.createElement("span");
    label.textContent = TEXT[segment.category] || TEXT.sponsor;
    const undo = document.createElement("button");
    undo.textContent = TEXT.undo;
    undo.style.cssText =
      "cursor:pointer;border:1px solid rgba(255,255,255,.5);background:transparent;color:#fff;" +
      "border-radius:8px;padding:4px 10px;font:inherit";
    const shown = note;
    undo.addEventListener("click", (event) => {
      event.stopPropagation();
      video.currentTime = segment.start;
      shown.remove();
    });
    note.append(label, undo);
    player.append(note);
    noteTimer = setTimeout(() => shown.remove(), 5000);
  };

  const onTime = (event) => {
    const video = event.target;
    if (!(video instanceof HTMLVideoElement) || inAd(video)) return;
    const id = videoId();
    if (!id) return;
    if (id !== current) {
      load(id);
      return;
    }
    const t = video.currentTime;
    for (const s of segments) {
      if (done.has(s.id) || t < s.start || t >= s.end - END_MARGIN) continue;
      done.add(s.id);
      video.currentTime = Math.min(s.end, video.duration || s.end);
      showNote(video, s);
      return;
    }
  };

  // The promoted videos and ad cards of the pages (home, search, next to the
  // video, mobile site), hidden whatever the lists say: the card is taken
  // out with its slot, so the grid leaves no hole.
  const HIDDEN = [
    "ytd-ad-slot-renderer",
    "ytd-rich-item-renderer:has(> #content > ytd-ad-slot-renderer)",
    "ytd-rich-section-renderer:has(ytd-ad-slot-renderer, ytd-statement-banner-renderer)",
    "ytd-in-feed-ad-layout-renderer",
    "ytd-promoted-sparkles-web-renderer",
    "ytd-promoted-sparkles-text-search-renderer",
    "ytd-promoted-video-renderer",
    "ytd-search-pyv-renderer",
    "ytd-display-ad-renderer",
    "ytd-compact-promoted-video-renderer",
    "ytd-companion-slot-renderer",
    "ytd-player-legacy-desktop-watch-ads-renderer",
    "ytd-banner-promo-renderer",
    "ytd-statement-banner-renderer",
    "ytd-video-masthead-ad-v3-renderer",
    "ytd-primetime-promo-renderer",
    "#masthead-ad",
    "#player-ads",
    "ytm-promoted-sparkles-web-renderer",
    "ytm-companion-ad-renderer",
    "ytm-rich-item-renderer:has(ad-slot-renderer)",
    "ad-slot-renderer",
  ];
  // A constructed sheet: YouTube's policy (`style-src`) empties a `<style>`
  // the page adds, not a sheet adopted by the document. Adopted again when
  // the page replaces the list.
  let sheet = null;
  const adopt = () => {
    try {
      if (!sheet) {
        sheet = new CSSStyleSheet();
        sheet.replaceSync(HIDDEN.join(",\n") + " { display: none !important; }");
      }
      if (!document.adoptedStyleSheets.includes(sheet)) {
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      }
    } catch (e) {}
  };

  // Media events don't bubble but are caught on the document.
  document.addEventListener("timeupdate", onTime, true);
  // The next video's segments, asked for as soon as YouTube shows its page.
  document.addEventListener("yt-navigate-finish", () => {
    adopt();
    const id = videoId();
    if (id && id !== current) load(id);
  });
  adopt();
  document.addEventListener("DOMContentLoaded", adopt);
})();
