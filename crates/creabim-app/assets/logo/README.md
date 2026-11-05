# The CreaBIM logo

The weaver and its nest (Brief 02, addendum A2). The weaver, native to Cameroon and much of Africa, is nature's architect. It perches on a branch beside the woven nest it built: maker and made, the designer and the model.

| File | Use |
| --- | --- |
| `svg/creabim-mark.svg`, `-small.svg` | The mark on a transparent background (sage), full and small cut |
| `svg/creabim-mark-ink.svg`, `-white.svg` (and `-small-…`) | One-colour versions |
| `svg/creabim-app-icon*.svg` | App tiles: sage, and dark |
| `svg/creabim-lockup.svg`, `-white.svg` | Mark plus the "creabim" wordmark (Nunito ExtraBold, SIL OFL) |
| `png/…` | Ready-made sizes. 32 px and below use the small cut |
| `creabim.ico` | Windows: 16 to 256 |
| `creabim.icns` | macOS: 16 to 1024 |

Where the app uses it:

- the top bar's brand and the CreaBIM menu: the small cut, 20 px (`ui/icons.js`, `creabim-mark`);
- the New project window: the full mark, 48 px (`creabim-mark-full`);
- About CreaBIM: the lockup (white on dark themes);
- the window and taskbar: `png/creabim-app-icon-256.png`;
- the Windows .exe: `creabim.ico`, embedded by `build.rs`.

**Rules:**

- Use the full mark only at 48 px and above, and the small cut below that.
- Leave clear space of at least a third of the nest's width all round.
- Never stretch, outline or shadow the mark.
- Use only these colours: sage `#4F6B4C`, deep sage `#3E5A3B`, dark `#1C211B`, light sage `#8FB08A`, ink `#22261F` and white.
