# README media

`neati-intro.gif` is rendered from `neati-intro.html`, a deterministic canvas
animation: `window.render(t)` draws the frame at `t` seconds (0–23), and opening
the file with `?t=<seconds>` shows one still frame.

To re-render, screenshot each frame at 30 fps with headless Chrome
(`--headless=new --allow-file-access-from-files --window-size=1280,720 --screenshot=frames/fNNNN.png
"file://.../neati-intro.html?t=<n/30>"`) and encode:

```bash
ffmpeg -framerate 30 -i frames/f%04d.png -vf "fps=12,scale=800:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" -loop 0 neati-intro.gif
```

The tile gradient and the mark path come from `src-tauri/icons/app-icon.svg`
and `src-tauri/icons/neati-mark.svg`; update the animation if the brand changes.

`clean.png`, `home.png` and `ai.png` are the screens shown in the window. They
are captured at 1180×760 (2x) from the browser preview (`just dev-web`), so the
numbers are the preview's sample data, not a real machine; the preview's
"Preview data" chip is hidden for the capture. Recapture them when the
interface changes.
