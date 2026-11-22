# README media

`neati-intro.gif` is rendered from `neati-intro.html`, a deterministic canvas
animation: `window.render(t)` draws the frame at `t` seconds (0–13), and opening
the file with `?t=<seconds>` shows one still frame.

To re-render, screenshot each frame at 30 fps with headless Chrome
(`--headless=new --window-size=1280,720 --screenshot=frames/fNNNN.png
"file://.../neati-intro.html?t=<n/30>"`) and encode:

```bash
ffmpeg -framerate 30 -i frames/f%04d.png -vf "fps=15,scale=800:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" -loop 0 neati-intro.gif
```

The tile gradient and the mark path come from `src-tauri/icons/app-icon.svg`
and `src-tauri/icons/neati-mark.svg`; update the animation if the brand changes.
