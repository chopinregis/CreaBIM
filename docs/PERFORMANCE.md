# Performance

CreaBIM should feel instant: no frame waits for a drawing, the app uses no CPU when left alone, and the download stays
small. This page records what was measured, what changed, and what is still to do (Brief 02, Addendum A6).

All figures are from the owner's Windows machine (16 cores) and the starter house: 640 elements, about 57,000
triangles.

## Where it stands

| What | Before A6 (main `0e5ed46`) | Now (`a6-performance`) | Target |
| --- | --- | --- | --- |
| CPU with the app left alone | 50–57 % of one core | 0.0–0.3 % (0.3–0.5 % with the WebView2 processes) | under 1 % |
| `creabim-app.exe` | 38.5 MB | 27.9 MB | under 20 MB |
| Window on screen (warm start) | 190–360 ms | 190 ms | under 300 ms |
| Selecting something with 3D open | about 80 ms (scene rebuilt, edges found again) | a scene copy (3 ms), recolouring and the upload | under 16 ms |
| A drag with sections open | every frame rebuilt every section (300–450 ms a frame) | sections and 3D build on workers; the frame never waits | 16 ms a frame |
| Section A-A | 224 ms | 73 ms | 30 ms |
| South / East Elevation | 333 / 455 ms | 89 / 113 ms | 30 ms |
| 3D Street view (a 3D drawing) | about 1,100 ms | 165 ms | 100 ms |
| A plan | 15 ms | 10–16 ms | 15 ms |

## What changed

1. **Idle means idle.** The web panes wake the app when they send a message; the 30 ms repaint poll is gone on Windows
   and macOS (Linux keeps it, as GTK only runs while frames pump it).
2. **Release profile:** fat LTO, one codegen unit, stripped, abort on panic. `cargo build --profile fast` gives
   near-release speed with a quick link for day-to-day testing.
3. **WebView data** lives in the user's local app data, not beside the program (an installed copy can't write
   there).
4. **One shared 3D scene per model revision** (`creabim_geom::shared_scene`). The 3D view, every section and
   elevation, and the tools use it instead of each building their own. Model revisions are unique across the
   process, so a cache can key on the revision alone.
5. **The 3D view's edges are cached by mesh content.** A selection change or a one-element edit doesn't find every
   element's edges again (32 ms → 1.4 ms).
6. **Drawings build off the UI thread** (`crates/creabim-app/src/builds.rs`). A view whose drawing takes 20 ms or more
   is rebuilt on a worker when the model changes. Its area keeps the last finished drawing until the new one arrives.
   The 3D view does the same with the scene. Plans are still built on the spot.
7. **Hidden lines on every core.** The depth buffer is built in bands of rows, one per core, each row only over the
   triangle's span. The edges are sampled per element in parallel. A test checks the banded buffer against a
   whole-box scan cell for cell.

## Still to do

- **A6-1.1 the live overlay:** during a drag, only the dragged elements redrawn in every area at 60 fps. Today the
  other areas update as fast as their background builds (about 10 a second for a section).
- **A6-2.2 depth buffer size from the view:** a fixed 2,048 cells across, whatever the view's size on screen.
- **A6-2.3 fewer edges:** 48,000 feature edges for a small house, most of them seams on flat faces; merge them
  before sampling.
- **A6-2.4 per-element caching:** rebuild only the elements a change touches.
- **A6-5 cached tessellation** of heavy drawings while the camera doesn't move.
- **A6-6.2 graphics back ends:** only what each platform needs. Also compress the help pictures.
- **Memory:** about 410 MB for the app with the starter house. Not traced yet.
- **Start-up:** the first launch builds every open view's drawing on the spot (its cost isn't known yet), so the app
  is busy for a few seconds after the window appears. Build the first drawings on workers too, with a placeholder.
- **Run-to-run differences:** some hidden-line drawings differ slightly between runs. Some data is kept in hash maps
  keyed by element IDs, so the scene's triangle order can change between launches, and equal-depth ties then resolve
  differently. Sort those maps, or break ties by element order, to make drawings repeatable.

## Measuring

```bash
# drawing and scene budgets (fails when a drawing is clearly slower than recorded)
cargo test --release -p creabim-geom --test perf -- --ignored --nocapture

# the background builds never make a frame wait
cargo test -p creabim-app builds::
```

To check idle CPU, start the release build, leave it alone for 10 seconds, and read Task Manager's CPU column for
`creabim-app.exe` (or use `top` on Linux and macOS).
