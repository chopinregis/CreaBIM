# CreaBIM website

The public site at https://chopinregis.github.io/CreaBIM/, served by GitHub Pages from this `gh-pages` branch.

Rebuild after a release or a change to the in-app guide:

    python _build/build_site.py --creabim <path to a CreaBIM checkout> --ref official/main

The guide pages are generated from `crates/creabim-app/help` on that ref; downloads come from the GitHub releases (`gh` CLI). Page text for home, features, download and tutorials lives in `_build/build_site.py`; styles in `assets/site.css`.
