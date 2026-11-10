"""Build the CreaBIM website (GitHub Pages, branch gh-pages).

    python _build/build_site.py --creabim C:/Dev/creabim --ref official/main

Reads the in-app guide (crates/creabim-app/help/*.md, its pictures and the
page order and key list in src/help.rs) from the given git ref of a CreaBIM
checkout, without touching that checkout's working tree, and the latest
releases from GitHub (`gh release`). Writes the site into this folder:
index.html, features.html, download.html, tutorials/, guide/, 404.html.
"""
import argparse
import html
import json
import re
import subprocess
import sys
from pathlib import Path

SITE = Path(__file__).resolve().parent.parent
REPO = "chopinregis/CreaBIM"
GH = f"https://github.com/{REPO}"
SITE_URL = "https://chopinregis.github.io/CreaBIM/"


# ---------------------------------------------------------------- sources

def git_show(repo: Path, ref: str, path: str, binary=False):
    out = subprocess.run(["git", "-C", str(repo), "show", f"{ref}:{path}"], capture_output=True, check=True)
    return out.stdout if binary else out.stdout.decode("utf-8")


def git_ls(repo: Path, ref: str, path: str):
    out = subprocess.run(["git", "-C", str(repo), "ls-tree", "--name-only", f"{ref}:{path}"], capture_output=True, check=True)
    return out.stdout.decode().split()


def releases():
    """The published releases, newest first, with their files."""
    cache = SITE / "_build" / "releases.json"
    try:
        out = subprocess.run(["gh", "api", f"repos/{REPO}/releases"], capture_output=True, check=True)
        data = json.loads(out.stdout)
        rel = [{"tag": r["tag_name"], "name": r["name"], "date": r["published_at"][:10], "url": r["html_url"],
                "assets": [{"name": a["name"], "size": a["size"], "url": a["browser_download_url"]} for a in r["assets"]]}
               for r in data if not r["draft"] and not r["prerelease"]]
        cache.write_text(json.dumps(rel, indent=1), encoding="utf-8")
        return rel
    except Exception as e:  # offline: use the last fetch
        print("gh release fetch failed, using cache:", e, file=sys.stderr)
        return json.loads(cache.read_text(encoding="utf-8"))


# ---------------------------------------------------------------- markdown (the guide's subset)

def slug(s: str) -> str:
    s = re.sub(r"<[^>]+>", "", s).lower()
    s = re.sub(r"[^a-z0-9 \-]", "", s)
    return re.sub(r"\s+", "-", s.strip())


KEYWORDS = {"OpenBracket": "[", "CloseBracket": "]", "PgUp": "Page Up", "PgDn": "Page Down", "PageUp": "Page Up", "PageDown": "Page Down"}


def keycaps(k: str) -> str:
    parts = []
    for p in re.split(r"(?<=\S)\+(?=\S)", k):
        p = KEYWORDS.get(p, p)
        parts.append(f"<kbd>{html.escape(p)}</kbd>")
    return "+".join(parts)


class Md:
    def __init__(self, pages: dict, version: str):
        self.pages = pages  # id -> title
        self.version = version

    def link(self, target: str) -> str:
        if re.match(r"^[a-z]+:", target):
            return target
        if target.startswith("#"):
            return target
        page, _, anchor = target.partition("#")
        page = page.removesuffix(".md")
        if page not in self.pages:
            print(f"  warning: link to unknown page {target!r}", file=sys.stderr)
        return f"{page}.html" + (f"#{anchor}" if anchor else "")

    def inline(self, s: str) -> str:
        s = s.replace("{version}", self.version).replace("{build}", self.version)
        codes = []

        def code(m):
            t = m.group(1)
            if "/" in t or "\\" in t or len(t) > 22 or t.startswith("<") or "." in t.strip(".") and not re.match(r"^[\w+]+$", t):
                codes.append(f"<code>{html.escape(t)}</code>")
            else:
                codes.append(keycaps(t))
            return f"\x00{len(codes) - 1}\x00"

        s = re.sub(r"`([^`]+)`", code, s)
        s = html.escape(s, quote=False)
        s = re.sub(r"!\[([^\]]*)\]\(([^)]+)\)", lambda m: self.img_inline(m.group(1), m.group(2)), s)

        def lk(m):
            href = self.link(m.group(2))
            ext = ' rel="noopener"' if href.startswith("http") else ""
            return f'<a href="{html.escape(href)}"{ext}>{m.group(1)}</a>'

        s = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", lk, s)
        s = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", s)
        s = re.sub(r"(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])", r"<em>\1</em>", s)
        s = re.sub(r"\x00(\d+)\x00", lambda m: codes[int(m.group(1))], s)
        return s

    def img_src(self, src):
        name = src.split("/")[-1]
        return f"../assets/help/{name}"

    def img_inline(self, alt, src):
        return f'<img src="{self.img_src(src)}" alt="{alt}" loading="lazy">'

    def figure(self, alt, src):
        if "creabim-lockup" in src:
            return ('<p class="lockup" role="img" aria-label="CreaBIM: the weaver and its nest" style="display:flex;align-items:center;gap:10px;'
                    'font:800 40px var(--head);color:var(--sage-deep);letter-spacing:-1px"><svg style="width:64px;height:64px;color:var(--sage)" aria-hidden="true">'
                    '<use href="#mark"/></svg>creabim</p>')
        return f'<figure><img src="{self.img_src(src)}" alt="{html.escape(alt)}" loading="lazy"><figcaption>{html.escape(alt)}</figcaption></figure>'

    def render(self, text: str) -> tuple[str, str, list]:
        lines = text.replace("\r\n", "\n").split("\n")
        out, toc = [], []
        title = ""
        i = 0
        para = []

        def flush():
            if para:
                out.append(f"<p>{self.inline(' '.join(x.strip() for x in para))}</p>")
                para.clear()

        while i < len(lines):
            ln = lines[i]
            st = ln.strip()
            if not st:
                flush(); i += 1; continue
            if st.startswith("<!-- upcoming"):
                flush(); out.append('<div class="soonbox">'); i += 1; continue
            if st.startswith("<!-- /upcoming"):
                flush(); out.append("</div>"); i += 1; continue
            if st.startswith("<!--"):
                while i < len(lines) and "-->" not in lines[i]:
                    i += 1
                i += 1; continue
            m = re.match(r"^(#{1,4})\s+(.*)$", st)
            if m:
                flush()
                lvl, t = len(m.group(1)), m.group(2)
                if lvl == 1:
                    title = t
                    out.append(f"<h1>{self.inline(t)}</h1>")
                else:
                    sid = slug(t)
                    if lvl == 2:
                        toc.append((sid, t))
                    out.append(f'<h{lvl} id="{sid}">{self.inline(t)}</h{lvl}>')
                i += 1; continue
            m = re.match(r"^!\[([^\]]*)\]\(([^)]+)\)$", st)
            if m:
                flush(); out.append(self.figure(m.group(1), m.group(2))); i += 1; continue
            if st.startswith("|"):
                flush()
                rows = []
                while i < len(lines) and lines[i].strip().startswith("|"):
                    rows.append(lines[i].strip()); i += 1
                out.append(self.table(rows)); continue
            if st.startswith(">"):
                flush()
                q = []
                while i < len(lines) and lines[i].strip().startswith(">"):
                    q.append(lines[i].strip()[1:].strip()); i += 1
                out.append(f'<div class="tip"><p>{self.inline(" ".join(q))}</p></div>'); continue
            if re.match(r"^\s*([-*]|\d+\.)\s+", ln):
                flush()
                block = []
                while i < len(lines) and lines[i].strip() and (re.match(r"^\s*([-*]|\d+\.)\s+", lines[i]) or lines[i].startswith("  ")):
                    block.append(lines[i]); i += 1
                out.append(self.lists(block)); continue
            para.append(ln); i += 1
        flush()
        return title, "\n".join(out), toc

    def table(self, rows):
        cells = lambda r: [c.strip() for c in r.strip("|").split("|")]
        head = cells(rows[0])
        body = [cells(r) for r in rows[1:] if not re.match(r"^\|[\s:\-|]+\|$", r)]
        h = "".join(f"<th>{self.inline(c)}</th>" for c in head)
        b = "".join("<tr>" + "".join(f"<td>{self.inline(c)}</td>" for c in r) + "</tr>" for r in body)
        return f'<div class="table-wrap"><table><thead><tr>{h}</tr></thead><tbody>{b}</tbody></table></div>'

    def lists(self, block):
        # items: (indent, ordered, text)
        items = []
        for ln in block:
            m = re.match(r"^(\s*)([-*]|\d+\.)\s+(.*)$", ln)
            if m:
                items.append([len(m.group(1)), m.group(2)[-1] == ".", m.group(3)])
            elif items:
                items[-1][2] += " " + ln.strip()

        def build(idx, indent):
            ordered = items[idx][1]
            tag = "ol" if ordered else "ul"
            parts = [f"<{tag}>"]
            while idx < len(items) and items[idx][0] >= indent:
                if items[idx][0] > indent:
                    sub, idx = build(idx, items[idx][0])
                    parts[-1] = parts[-1].removesuffix("</li>") + sub + "</li>"
                    continue
                parts.append(f"<li>{self.inline(items[idx][2])}</li>")
                idx += 1
            parts.append(f"</{tag}>")
            return "".join(parts), idx

        html_out, n = build(0, items[0][0])
        while n < len(items):
            more, n = build(n, items[n][0])
            html_out += more
        return html_out


# ---------------------------------------------------------------- layout

ICONS = """<svg width="0" height="0" style="position:absolute" aria-hidden="true"><defs>
<symbol id="mark" viewBox="0 0 200 200"><g transform="translate(9 4)"><path d="M26 52 H174" fill="none" stroke="currentColor" stroke-width="9" stroke-linecap="round"/><path d="M130 52 C162 64 172 100 166 128 C160 156 146 166 142 190 L118 190 C114 166 98 156 94 128 C88 100 98 64 130 52 Z" fill="currentColor"/><path d="M98 98 L164 124 M96 132 L160 156" fill="none" stroke="var(--paper)" stroke-width="9" stroke-linecap="round"/><path d="M44.5 8.0 C51.2 2.1 63.8 3.0 67.2 12.2 C69.7 18.9 66.4 24.8 63.0 29.0 C61.3 39.9 52.9 50.0 38.6 50.8 C32.8 51.3 27.7 50.0 23.5 48.3 L8.4 56.7 L16.8 43.3 C14.3 35.7 16.0 27.3 21.8 22.3 C27.7 17.2 37.8 14.7 44.5 8.0 Z M65.5 10.5 L80.6 16.4 L65.5 21.4 Z" fill="currentColor"/><circle cx="61.5" cy="11.4" r="2.8" fill="var(--paper)"/></g></symbol>
<symbol id="i-dl" viewBox="0 0 24 24"><path d="M12 3v12m0 0-5-5m5 5 5-5M4 19h16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></symbol>
<symbol id="i-gh" viewBox="0 0 24 24"><path d="M12 2a10 10 0 0 0-3.2 19.5c.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.3-3.4-1.3-.5-1.1-1.1-1.4-1.1-1.4-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.4 1.1 2.9.8.1-.6.3-1.100.6-1.3-2.2-.3-4.6-1.1-4.6-5a3.9 3.9 0 0 1 1-2.7c-.1-.3-.4-1.3.1-2.7 0 0 .8-.3 2.8 1a9.6 9.6 0 0 1 5 0c1.9-1.3 2.8-1 2.8-1 .5 1.4.2 2.4.1 2.7a3.9 3.9 0 0 1 1 2.7c0 3.9-2.4 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10 10 0 0 0 12 2z" fill="currentColor"/></symbol>
<symbol id="i-sun" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="2"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></symbol>
<symbol id="i-moon" viewBox="0 0 24 24"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></symbol>
<symbol id="i-menu" viewBox="0 0 24 24"><path d="M4 7h16M4 12h16M4 17h16" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></symbol>
<symbol id="i-off" viewBox="0 0 24 24"><path d="M4 7h16v10H4zM8 21h8M12 17v4" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></symbol>
<symbol id="i-file" viewBox="0 0 24 24"><path d="M6 2h8l4 4v16H6zM14 2v4h4M9 13h6M9 17h6" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></symbol>
<symbol id="i-code" viewBox="0 0 24 24"><path d="m8 7-5 5 5 5M16 7l5 5-5 5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></symbol>
<symbol id="i-cube" viewBox="0 0 24 24"><path d="M12 2 3 7v10l9 5 9-5V7zM3 7l9 5 9-5M12 12v10" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></symbol>
<symbol id="i-sheet" viewBox="0 0 24 24"><path d="M3 4h18v16H3zM3 16h18M14 16v4M7 8h6M7 11h4" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/></symbol>
<symbol id="i-swap" viewBox="0 0 24 24"><path d="M4 8h14l-4-4M20 16H6l4 4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></symbol>
<symbol id="i-book" viewBox="0 0 24 24"><path d="M4 4h6a3 3 0 0 1 3 3v13a2 2 0 0 0-2-2H4zM20 4h-6a3 3 0 0 0-3 3v13a2 2 0 0 1 2-2h7z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></symbol>
<symbol id="i-play" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" stroke-width="2"/><path d="m10 8 6 4-6 4z" fill="currentColor"/></symbol>
<symbol id="i-win" viewBox="0 0 24 24"><path d="M3 5.5 10.5 4.5v7H3zM12 4.3 21 3v8.5h-9zM3 12.5h7.5v7L3 18.5zM12 12.5h9V21l-9-1.3z" fill="currentColor"/></symbol>
<symbol id="i-linux" viewBox="0 0 24 24"><path d="M12 2c-2.2 0-3.5 1.8-3.5 4.3 0 1.5.3 2.4-.6 3.9C6.6 12.3 5 14.400 5 17c0 1 .4 1.8 1 2.300-.6.4-1.300.8-1.300 1.4C4.700 21.700 6 22 7.500 22c1.300 0 2.300-.6 3-1.100.5.1 1 .1 1.500.1s1-.1 1.500-.1c.700.500 1.700 1.100 3 1.100 1.500 0 2.800-.3 2.800-1.300 0-.6-.700-1-1.300-1.400.600-.5 1-1.300 1-2.300 0-2.600-1.600-4.700-2.900-6.800-.900-1.500-.6-2.400-.6-3.900C15.500 3.800 14.200 2 12 2z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><circle cx="10.300" cy="7" r=".9" fill="currentColor"/><circle cx="13.700" cy="7" r=".9" fill="currentColor"/><path d="M10.500 9.300 12 10.300l1.500-1" fill="none" stroke="currentColor" stroke-width="1.500" stroke-linecap="round" stroke-linejoin="round"/></symbol>
<symbol id="i-shield" viewBox="0 0 24 24"><path d="M12 2 4 5v6c0 5 3.400 9.300 8 11 4.600-1.700 8-6 8-11V5zM8.500 12l2.500 2.500 4.500-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/></symbol>
</defs></svg>"""

NAV = [("index.html", "Home"), ("features.html", "Features"), ("tutorials/index.html", "Tutorials"), ("guide/welcome.html", "Guide"), ("download.html", "Download")]


def layout(*, depth: int, current: str, title: str, desc: str, body: str, path: str) -> str:
    b = "../" * depth
    links = "".join(
        f'<a href="{b}{href}"{" aria-current=page" if label == current else ""}>{label}</a>' for href, label in NAV)
    full_title = "CreaBIM: free, open-source BIM modeller" if title is None else f"{title} · CreaBIM"
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{html.escape(full_title)}</title>
<meta name="description" content="{html.escape(desc)}">
<link rel="canonical" href="{SITE_URL}{path}">
<meta property="og:title" content="{html.escape(full_title)}">
<meta property="og:description" content="{html.escape(desc)}">
<meta property="og:type" content="website">
<meta property="og:url" content="{SITE_URL}{path}">
<meta property="og:image" content="{SITE_URL}assets/img/app-3d-and-plan.jpg">
<meta name="twitter:card" content="summary_large_image">
<meta name="theme-color" content="#F5F4EF" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#141712" media="(prefers-color-scheme: dark)">
<link rel="icon" type="image/png" sizes="64x64" href="{b}assets/logo/icon-64.png">
<link rel="apple-touch-icon" href="{b}assets/logo/icon-256.png">
<script>try{{var t=localStorage.getItem("creabim-theme");if(t)document.documentElement.dataset.theme=t}}catch(e){{}}</script>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Nunito:wght@700;800&family=Inter:wght@400;500;600&family=Instrument+Serif:ital@1&family=JetBrains+Mono:wght@400;500;600&display=swap">
<link rel="stylesheet" href="{b}assets/site.css">
</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
{ICONS}
<header class="nav gutter">
  <div class="wrap">
    <a class="brand" href="{b}index.html" aria-label="CreaBIM home"><svg aria-hidden="true"><use href="#mark"/></svg>creabim</a>
    <nav class="links" aria-label="Site">{links}</nav>
    <div class="end">
      <button class="ghost" id="theme" type="button" aria-label="Switch between light and dark"><svg class="i-sun"><use href="#i-sun"/></svg><svg class="i-moon"><use href="#i-moon"/></svg></button>
      <a class="btn gh" href="{GH}"><svg aria-hidden="true"><use href="#i-gh"/></svg>GitHub</a>
      <a class="btn primary dlbtn" href="{b}download.html"><svg aria-hidden="true"><use href="#i-dl"/></svg><span>Download</span></a>
      <button class="ghost" id="menu" type="button" aria-label="Menu" aria-expanded="false"><svg><use href="#i-menu"/></svg></button>
    </div>
  </div>
</header>
<main id="main">
{body}
</main>
<footer class="gutter">
  <div class="wrap">
    <a class="brand" href="{b}index.html"><svg aria-hidden="true"><use href="#mark"/></svg>creabim</a>
    <span>Free and open source · GPL-3.0-or-later</span>
    <nav aria-label="Footer"><a href="{b}features.html">Features</a><a href="{b}tutorials/index.html">Tutorials</a><a href="{b}download.html">Download</a><a href="{GH}">GitHub</a><a href="{GH}/issues">Report a bug</a><a href="{GH}/discussions">Discussions</a></nav>
  </div>
</footer>
<script src="{b}assets/site.js" defer></script>
</body>
</html>
"""


def fmt_size(n):
    return f"{n / 1_000_000:.1f} MB" if n >= 100_000 else f"{max(1, round(n / 1000))} KB" if n >= 1000 else f"{n} bytes"


# ---------------------------------------------------------------- pages

def home(rel):
    v = rel["tag"].lstrip("v")
    return f"""
  <section class="hero gutter">
    <div class="ident" id="ident" role="img" aria-label="The CreaBIM logo: a weaver beside the woven nest it built"></div>
    <a class="pill" href="download.html"><b>{v}</b>Free and open source<span class="pill-x"> · Windows and Linux</span></a>
    <h1>One model.<span>Every drawing.</span></h1>
    <p class="lede">CreaBIM is a free BIM modeller for architects. Model your building in 3D, and its plans, sections, elevations, sheets, schedules, quantities and IFC follow on their own: always in step, on your own computer, with no subscription.</p>
    <div class="cta">
      <a class="btn primary" href="download.html"><svg aria-hidden="true"><use href="#i-dl"/></svg>Download CreaBIM {v}</a>
      <a class="btn" href="tutorials/index.html"><svg aria-hidden="true"><use href="#i-book"/></svg>Start the tutorial</a>
    </div>
    <ul class="ticks"><li>No account, no licence fee</li><li>Works fully offline</li><li>IFC in and out</li><li>Blender-style keys</li></ul>
    <figure class="shot wrap" style="margin-inline:auto">
      <img src="assets/img/app-3d-and-plan.jpg" width="1920" height="1080" alt="CreaBIM with the Maple Lane House in 3D beside its ground-floor plan" fetchpriority="high">
      <figcaption><span class="live">Maple Lane House · the starter project</span><span>3D and plan side by side · 1:50</span></figcaption>
    </figure>
  </section>

  <section class="proof" aria-label="At a glance">
    <ul class="wrap gutter">
      <li><b>640<small>elements</small></b><span>in the starter house, footings to fixtures</span></li>
      <li><b>11<small>sheets</small></b><span>drawn from one model, as vector PDF</span></li>
      <li><b>3<small>IFC schemas</small></b><span>IFC2X3, IFC4 and IFC4X3 read</span></li>
      <li><b>1<small>model</small></b><span>behind every view, sheet, schedule and export</span></li>
    </ul>
  </section>

  <section class="sec gutter" id="seam">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">01</span>The seam</div>
        <h2>Design in one tool. Document in <span class="acc">another.</span></h2>
        <p class="lede">Most practices run two programs and a handover between them: a modeller for the design, a drafting or BIM tool for the permit set. Every revision crosses that boundary by hand, so the drawing you submit is only as current as the last time someone updated it.</p>
      </div>
      <div class="two">
        <div class="panel">
          <div class="ph"><span>The usual pipeline</span><span>two tools, one seam</span></div>
          <ol class="steps">
            <li><span class="n">01</span><span class="t">Concept massing</span><span class="s">design tool</span></li>
            <li><span class="n">02</span><span class="t">Design development</span><span class="s">design tool</span></li>
            <li class="dup"><span class="n">03</span><span class="t">Redraw the plans</span><span class="s">CAD</span></li>
            <li class="dup"><span class="n">04</span><span class="t">Rebuild in BIM</span><span class="s">authoring</span></li>
            <li class="dup"><span class="n">05</span><span class="t">Reconcile the set</span><span class="s">each revision</span></li>
          </ol>
          <p class="foot-note">Three of these five steps exist only because the model and the drawings live in different files.</p>
        </div>
        <div class="panel">
          <div class="ph"><span>CreaBIM</span><span>one store, many views</span></div>
          <div class="hub"><b>One element store</b><span>walls · slabs · openings · levels · grids · rooms</span></div>
          <ul class="fan">
            <li>3D view <span>live</span></li><li>Plan <span>1:50</span></li><li>Section <span>cut</span></li><li>Elevation <span>1:100</span></li>
            <li>Sheet <span>A1</span></li><li>IFC <span>4</span></li><li>Schedule <span>csv</span></li><li>Take-off <span>xlsx</span></li>
          </ul>
          <p class="foot-note">One edit lands in all of them, in the same moment and the same undo step.</p>
        </div>
      </div>
    </div>
  </section>

  <div class="night"><section class="sec gutter" id="speed">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">02</span>Performance</div>
        <h2>Small, fast, and <span class="acc">quiet.</span></h2>
        <p class="lede">CreaBIM is written in Rust and draws on your graphics card. Sections and 3D drawings are built on every core in the background, so dragging a wall never waits for the other views.</p>
      </div>
      <ul class="stats">
        <li><b>16<small>MB</small></b><span>The Windows installer</span></li>
        <li><b>0.2<small>s</small></b><span>From double-click to a window</span></li>
        <li><b>0.3<small>%</small></b><span>CPU when you leave it alone</span></li>
        <li><b>16<small>cores</small></b><span>Used to draw sections and 3D views</span></li>
      </ul>
      <p class="stats-note">Measured with the starter house on a 16-core Windows desktop. Your numbers will vary with your machine and your model.</p>
    </div>
  </section></div>

  <section class="sec gutter" id="drawings">
    <div class="wrap two mid">
      <div class="stack">
        <div class="head" style="margin:0">
          <div class="kicker"><span class="n">03</span>Drawing quality</div>
          <h2>Drawn from the model, not <span class="acc">traced over it.</span></h2>
          <p class="lede">Sections and elevations are generated from the model. Cut walls, slabs and openings are drawn heavy; what lies beyond is projected with hidden lines removed.</p>
        </div>
        <ul class="checks">
          <li>Draw sections and elevations in a plan, or inside another section</li>
          <li>Level datums, section and elevation marks, door swings and window symbols</li>
          <li>Sheets with a title block, scales and live views, exported as vector PDF</li>
          <li>Shade any 3D view or elevation in material colours, on screen and on sheets</li>
        </ul>
        <div class="chips" aria-label="Exports">
          <span class="chip"><code>SVG</code>Plan drawings</span><span class="chip"><code>PDF</code>Sheets</span><span class="chip"><code>CSV</code>Schedules</span>
          <span class="chip"><code>XLSX</code>Schedules and take-off</span><span class="chip"><code>IFC</code>IFC4</span>
          <span class="chip"><code>GLB</code>glTF for Blender</span><span class="chip"><code>FCSTD</code>FreeCAD document</span>
        </div>
      </div>
      <figure class="clip">
        <div class="view"><img src="assets/img/plan-ground-floor.jpg" width="1100" height="1255" alt="Part of the Maple Lane House ground-floor plan, drawn by CreaBIM" loading="lazy"></div>
        <figcaption>Ground-floor plan, drawn by CreaBIM from the model · hatches, dimension chains, room tags</figcaption>
      </figure>
    </div>
  </section>

  <section class="sec gutter" id="profiles">
    <div class="wrap two mid">
      <figure class="clip">
        <div class="view"><img src="assets/img/profile-view.jpg" width="1400" height="850" alt="The Profile view: a profile with a void, being copied" loading="lazy"></div>
        <figcaption>The Profile view · draw a shape, pick a void, copy as you move</figcaption>
      </figure>
      <div class="stack first-mobile">
        <div class="head" style="margin:0">
          <div class="kicker"><span class="n">04</span>Profiles</div>
          <h2>Your own sections, <span class="acc">drawn once.</span></h2>
          <p class="lede">Draw a coping, a gutter or a steel section in its own view, with snaps, guides and typed lengths, then use it for sweeps, columns, beams and railings. Change it later, and the elements made with it take the new shape when you apply it.</p>
        </div>
        <ul class="checks">
          <li>Several shapes in one profile, with holes and voids you pick</li>
          <li>Trace over a picture, a DXF or a view of the project</li>
          <li>Move, rotate, align and copy, with box select</li>
        </ul>
      </div>
    </div>
  </section>

  <div class="night"><section class="sec gutter" id="keys">
    <div class="wrap keys">
      <div class="stack" style="gap:18px">
        <div class="kicker"><span class="n">05</span>Workflow</div>
        <h2 style="font-size:clamp(30px,4.6vw,52px)">If you know Blender, you already know <span class="acc">the keys.</span></h2>
        <p class="lede">Move, rotate and scale the way a modeller expects: press the key, move the pointer, type a value if you want it exact. Sections and plans take the same edits, so a drawing is the model, seen flat.</p>
        <ul class="legend">
          <li><kbd class="cap">G</kbd>Move with the pointer</li><li><kbd class="cap">R</kbd>Rotate</li>
          <li><kbd class="cap">S</kbd>Scale</li><li><kbd class="cap">E</kbd>Extrude a face or sketch</li>
          <li><kbd class="cap">⇧ D</kbd>Duplicate</li><li><kbd class="cap">H</kbd>Hide the selection</li>
          <li><kbd class="cap">Tab</kbd>Object or Edit mode</li><li><kbd class="cap">/</kbd>Show the selection alone</li>
          <li><kbd class="cap">⇧ A</kbd>Add</li><li><kbd class="cap">⌃ Z</kbd>Undo, always</li>
        </ul>
      </div>
      <div class="stack" style="gap:28px">
        <div class="keyrow"><h4>Transform</h4><div class="caps"><kbd class="cap">G</kbd><kbd class="cap">R</kbd><kbd class="cap">S</kbd><kbd class="cap">Shift D</kbd><kbd class="cap">E</kbd></div><div class="caps"><kbd class="cap">X</kbd><kbd class="cap">Y</kbd><kbd class="cap">Z</kbd><kbd class="cap">Tab</kbd><kbd class="cap">H</kbd><kbd class="cap">/</kbd></div></div>
        <div class="keyrow"><h4>Then type a value</h4><div class="caps"><kbd class="cap">2400</kbd><kbd class="cap">↵</kbd><kbd class="cap">Esc</kbd></div></div>
        <div class="keyrow"><h4>Snaps and units</h4><div class="caps"><kbd class="cap">F3 snaps</kbd><kbd class="cap">Alt S grid</kbd><kbd class="cap">mm</kbd></div></div>
        <a class="btn" href="guide/shortcuts.html" style="justify-self:start">Every shortcut</a>
      </div>
    </div>
  </section></div>

  <section class="sec gutter" id="open">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">06</span>Open by design</div>
        <h2>Your work stays <span class="acc">yours.</span></h2>
      </div>
      <div class="grid3">
        <div class="card"><span class="ico"><svg><use href="#i-off"/></svg></span><h3>Offline, on your machine</h3><p>No account, no cloud, no telemetry. Your projects are files on your disk.</p></div>
        <div class="card"><span class="ico"><svg><use href="#i-file"/></svg></span><h3>Open files</h3><p>An open, documented project format, IFC in and out, DXF references, glTF and FreeCAD exchange.</p></div>
        <div class="card"><span class="ico"><svg><use href="#i-code"/></svg></span><h3>Open source, GPL-3.0</h3><p>Read the code, build it yourself, report a bug or send a fix.</p></div>
      </div>
    </div>
  </section>

  <section class="sec gutter" id="next">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">07</span>Go further</div>
        <h2>See it, learn it, <span class="acc">try it.</span></h2>
      </div>
      <div class="grid3">
        <a class="card" href="features.html"><span class="ico"><svg><use href="#i-cube"/></svg></span><h3>Features</h3><p>Modelling, drawings, sheets, schedules and exchange, with where each part stands today.</p><span class="more">See the features →</span></a>
        <a class="card" href="tutorials/index.html"><span class="ico"><svg><use href="#i-book"/></svg></span><h3>Tutorials</h3><p>Build a first house in eight short steps, from an empty project to a printed sheet and an IFC file.</p><span class="more">Start the tutorial →</span></a>
        <a class="card" href="download.html"><span class="ico"><svg><use href="#i-dl"/></svg></span><h3>Download</h3><p>CreaBIM {v} for Windows and Linux: an installer, a portable zip, or the source.</p><span class="more">Get CreaBIM →</span></a>
      </div>
    </div>
  </section>

  <section class="sec gutter" id="faq">
    <div class="wrap" style="max-width:820px">
      <div class="head"><div class="kicker"><span class="n">08</span>Good to know</div></div>
      <details><summary>Is it really free?</summary><p>Yes. CreaBIM is open-source software under GPL-3.0: free to use, study, change and share.</p></details>
      <details><summary>Does it need the internet?</summary><p>No. It runs on your computer and never needs a connection.</p></details>
      <details><summary>Can I use it on real projects?</summary><p>Not yet for production work. It's ready to explore and to model with; check <a href="features.html#status">where it stands</a> before you rely on it.</p></details>
      <details><summary>Does it open Revit or ArchiCAD files?</summary><p>Through IFC: export IFC from those tools and open it in CreaBIM. Native .rvt files are a closed format.</p></details>
      <details><summary>Is there a Mac version?</summary><p>Not yet. Windows and Linux come first.</p></details>
    </div>
  </section>

  <section class="final gutter">
    <svg class="m" aria-hidden="true"><use href="#mark"/></svg>
    <h2>Model it once. Draw it <span class="acc">everywhere.</span></h2>
    <div class="cta">
      <a class="btn primary" href="download.html"><svg aria-hidden="true"><use href="#i-dl"/></svg>Download CreaBIM</a>
      <a class="btn" href="{GH}"><svg aria-hidden="true"><use href="#i-gh"/></svg>GitHub</a>
    </div>
  </section>
"""


FEATURES = [
    ("model", "i-cube", "Modelling", "Build it the way it is built.",
     "Real building elements that know what they are, drawn with snaps and guides in any view: plan, section, elevation or 3D.",
     [("Walls", "Layered assemblies, location lines, clean L, T and cross joins, flip in or out.", "walls"),
      ("Doors and windows", "They cut their own holes, flip hand and swing, and tag themselves.", "doors-windows"),
      ("Slabs and roofs", "From an outline or picked walls, with holes, slopes and arcs.", "slabs"),
      ("Columns, beams and footings", "Structure on grids and levels, with your own profiles.", "columns-beams"),
      ("Stairs and railings", "Straight runs and landings, with railings that follow.", "stairs-railings"),
      ("Curtain walls", "Grids of panels and mullions on a wall's path.", "curtain-walls"),
      ("Rooms and ceilings", "Rooms with areas and tags; ceilings from the room outline.", "rooms"),
      ("Masses and sketches", "Freeform volumes for concept work, extruded from a sketch.", "masses"),
      ("Furniture, fixtures and site", "Catalogue objects that sit against walls, and the ground around the building.", "furniture"),
      ("Types and materials", "Change a type once and every element of it follows.", "types")]),
    ("edit", "i-swap", "Editing", "Edit like a modeller.",
     "Blender-style keys for move, rotate and scale, typed values for exact work, and an Edit mode for points, edges and faces.",
     [("Move, rotate, scale", "Press the key, move the pointer, type a value.", "move"),
      ("Edit mode", "Pick points, edges or faces; push and pull a face.", "edit-mode"),
      ("Align and handles", "Align to any edge; drag grips in 2D and handles in 3D.", "align"),
      ("Snaps and guides", "Endpoints, midpoints, intersections and dashed guide lines.", "snaps"),
      ("Selecting", "Click, box select, select all of a type, isolate with /.", "select"),
      ("Undo, always", "Every change, in every view, is one undo step.", "edit")]),
    ("draw", "i-sheet", "Drawings and documents", "Drawings you can submit.",
     "Plans, sections and elevations are read from the model, with cut lines, hidden lines removed and the annotation an architect expects.",
     [("Plans, sections and elevations", "Live views, at any scale, cut where you draw them.", "sections"),
      ("View range and graphic styles", "Cut height, depth, line weights and hatches by category.", "graphic-styles"),
      ("Dimensions, tags and marks", "Dimension chains, door, window and room tags, level and section marks.", "dimensions"),
      ("Details", "Callouts with their own crop and scale.", "details"),
      ("Sheets", "Title blocks, scales and live views, exported as vector PDF.", "sheets"),
      ("Schedules and take-off", "Door, window and room schedules; quantities by material, to CSV and XLSX.", "take-off")]),
    ("files", "i-file", "Files and exchange", "Open formats, both ways.",
     "Your project is an open, documented file. Other tools read what CreaBIM writes, and CreaBIM reads theirs.",
     [("IFC", "Read IFC2X3, IFC4 and IFC4X3; write IFC4.", "ifc"),
      ("Blender", "glTF / GLB out to Blender, and a Blender import.", "blender"),
      ("FreeCAD", "Write a FreeCAD document.", "freecad"),
      ("DXF and SVG", "DXF in as a reference to trace over; plans out as SVG.", "dxf-svg"),
      ("Trace a reference", "A picture, a DXF or another view under the one you draw in.", "trace-reference"),
      ("The project file", "Plain, open JSON with a published schema.", "files")]),
]


def features():
    toc = "".join(f'<a href="#{k}">{t}</a>' for k, _, t, *_ in FEATURES) + '<a href="#status">Where it stands</a>'
    secs = []
    for n, (key, ico, t, h, lede, items) in enumerate(FEATURES, 1):
        li = "".join(f'<li><b>{a}</b><span>{b}</span><a href="guide/{c}.html">Read the guide →</a></li>' for a, b, c in items)
        secs.append(f"""
  <section class="sec gutter" id="{key}">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">0{n}</span>{t}</div>
        <h2>{h.rsplit(' ', 1)[0]} <span class="acc">{h.rsplit(' ', 1)[1]}</span></h2>
        <p class="lede">{lede}</p>
      </div>
      <ul class="feat-list">{li}</ul>
    </div>
  </section>""")
    pics = """
  <section class="sec gutter">
    <div class="wrap grid2">
      <figure class="clip"><div class="view"><img src="assets/img/sheet-ground-floor.jpg" alt="A CreaBIM sheet: the ground-floor plan with its title block" loading="lazy"></div><figcaption>A sheet, straight from the model</figcaption></figure>
      <figure class="clip"><div class="view"><img src="assets/img/profile-view.jpg" alt="The Profile view in CreaBIM" loading="lazy"></div><figcaption>The Profile view: your own sections for sweeps and beams</figcaption></figure>
    </div>
  </section>"""
    return f"""
  <section class="phead gutter">
    <div class="wrap" style="display:grid;gap:18px">
      <div class="kicker">Features</div>
      <h1>Everything follows from <span class="acc">one model.</span></h1>
      <p class="lede">What CreaBIM does today, what each part is for, and the guide page that shows you how. The same pages are inside the app: press <kbd>F1</kbd> over any tool.</p>
      <nav class="toc" aria-label="On this page">{toc}</nav>
    </div>
  </section>
  <section class="gutter"><figure class="shot wrap" style="margin:0 auto"><img src="assets/img/app-3d-and-plan.jpg" width="1920" height="1080" alt="CreaBIM: 3D and plan side by side" loading="lazy"><figcaption><span class="live">One model</span><span>every view updates as you edit</span></figcaption></figure></section>
  {''.join(secs[:2])}
  <div class="night"><section class="sec gutter">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">··</span>Under the hood</div>
        <h2>Small, fast, and <span class="acc">quiet.</span></h2>
        <p class="lede">Written in Rust and drawn on your graphics card. Sections and 3D drawings build on every core in the background; left alone, the app uses next to no CPU.</p>
      </div>
      <ul class="stats">
        <li><b>16<small>MB</small></b><span>The Windows installer</span></li>
        <li><b>0.2<small>s</small></b><span>From double-click to a window</span></li>
        <li><b>0.3<small>%</small></b><span>CPU when you leave it alone</span></li>
        <li><b>0<small>accounts</small></b><span>No sign-in, no cloud, no telemetry</span></li>
      </ul>
      <p class="stats-note">Measured with the starter house on a 16-core Windows desktop.</p>
    </div>
  </section></div>
  {''.join(secs[2:])}
  {pics}
  <section class="sec gutter status" id="status">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">05</span>Where it stands</div>
        <h2>Ready to explore, not yet for <span class="acc">production.</span></h2>
        <p class="lede">CreaBIM covers the whole path from model to sheets and IFC. Some parts are young, and we say which.</p>
      </div>
      <div class="two">
        <div class="panel"><h3>Works today</h3><ul><li>Walls, slabs, roofs, columns, beams, footings, doors and windows</li><li>Plans, sections, elevations, sheets and schedules from one model</li><li>Quantities and take-off</li><li>IFC4 in and out; glTF and FreeCAD exchange</li></ul></div>
        <div class="panel no"><h3>Still young</h3><ul><li>IFC export of some elements</li><li>Blender import and export</li><li>DXF (lines only)</li><li>Stairs (basic shapes)</li></ul><p class="foot-note">The full list is in <a href="{GH}/blob/main/docs/KNOWN_GAPS.md">Known gaps</a> on GitHub.</p></div>
      </div>
    </div>
  </section>
  <section class="final gutter">
    <h2>Try it on your own <span class="acc">project.</span></h2>
    <div class="cta"><a class="btn primary" href="download.html"><svg aria-hidden="true"><use href="#i-dl"/></svg>Download CreaBIM</a><a class="btn" href="tutorials/index.html"><svg aria-hidden="true"><use href="#i-book"/></svg>Start the tutorial</a></div>
  </section>
"""


def download(rels):
    r = rels[0]
    v = r["tag"].lstrip("v")
    A = {a["name"]: a for a in r["assets"]}
    pick = lambda suffix: next((a for n, a in A.items() if n.endswith(suffix)), None)
    setup, zipf, tar, sums = pick("-setup.exe"), pick("windows-x64.zip"), pick("linux-x64.tar.gz"), pick("SHA256SUMS.txt")
    rows = "".join(f'<tr><td><a href="{a["url"]}">{html.escape(a["name"])}</a></td><td class="num">{fmt_size(a["size"])}</td></tr>' for a in r["assets"])
    older = "".join(f'<tr><td><a href="{o["url"]}">{html.escape(o["name"])}</a></td><td class="num"><a href="{o["url"]}">Files ↗</a></td></tr>' for o in rels[1:]) or '<tr><td colspan="2">None yet.</td></tr>'
    zipline = f'<p class="alt">No installer? <a href="{zipf["url"]}">Portable zip</a> ({fmt_size(zipf["size"])}): unzip and run CreaBIM.exe.</p>' if zipf else ""
    return f"""
  <section class="phead gutter">
    <div class="wrap" style="display:grid;gap:18px">
      <div class="kicker">Download</div>
      <h1>Run it on your <span class="acc">desk.</span></h1>
      <p class="lede">CreaBIM is free, with no account and no licence key. Download it, open it, and it starts on a finished starter house to explore.</p>
    </div>
  </section>
  <section class="gutter" style="padding-bottom:72px">
    <div class="wrap" style="display:grid;gap:26px">
      <div class="release"><h2>CreaBIM {v}</h2><span class="badge">Latest</span><span class="mono">{r["name"].split("·")[-1].strip()}</span><a class="mono" href="{r["url"]}">Release notes on GitHub ↗</a></div>
      <div class="dl">
        <div class="card">
          <div class="os"><svg aria-hidden="true"><use href="#i-win"/></svg><h3>Windows</h3></div>
          <p>Windows 10 or 11, 64-bit. The installer adds CreaBIM to the Start menu.</p>
          <a class="btn primary" href="{setup["url"]}"><svg aria-hidden="true"><use href="#i-dl"/></svg>Installer <small>{fmt_size(setup["size"])}</small></a>
          {zipline}
        </div>
        <div class="card">
          <div class="os"><svg aria-hidden="true"><use href="#i-linux"/></svg><h3>Linux</h3></div>
          <p>64-bit. Untar and run <code>./creabim</code>. Needs WebKitGTK 4.1 and GTK 3.</p>
          <a class="btn primary" href="{tar["url"]}"><svg aria-hidden="true"><use href="#i-dl"/></svg>tar.gz <small>{fmt_size(tar["size"])}</small></a>
          <p class="alt">On Ubuntu: <code>sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0</code></p>
        </div>
        <div class="card">
          <div class="os"><svg aria-hidden="true"><use href="#i-code"/></svg><h3>From source</h3></div>
          <p>Any platform Rust and wgpu run on. You need Rust stable.</p>
          <a class="btn" href="{GH}"><svg aria-hidden="true"><use href="#i-gh"/></svg>Source on GitHub</a>
          <p class="alt"><code>cargo run --release -p creabim-app</code></p>
        </div>
      </div>
      <div class="note"><b>Windows says the publisher is unknown?</b><span>CreaBIM isn't code-signed yet. Choose <em>More info</em>, then <em>Run anyway</em>. You can check the file against its SHA-256 checksum first (below).</span></div>
    </div>
  </section>
  <section class="sec gutter" style="padding-top:0;border:0">
    <div class="wrap two">
      <div class="stack">
        <h2 style="font-size:28px">Every file in {v}</h2>
        <div class="table-wrap"><table><thead><tr><th>File</th><th style="text-align:right">Size</th></tr></thead><tbody>{rows}</tbody></table></div>
        <h2 style="font-size:28px;margin-top:12px">Earlier releases</h2>
        <div class="table-wrap"><table><thead><tr><th>Release</th><th style="text-align:right">Downloads</th></tr></thead><tbody>{older}</tbody></table></div>
      </div>
      <div class="stack">
        <h2 style="font-size:28px">Check your download</h2>
        <p class="lede" style="font-size:16px">Each release lists the SHA-256 checksum of every file in <a href="{sums["url"] if sums else r["url"]}">SHA256SUMS.txt</a>. Compare it with the checksum of the file you downloaded:</p>
        <pre class="term">Windows (PowerShell)
  Get-FileHash .\\{html.escape(setup["name"])}

Linux
  sha256sum {html.escape(tar["name"])}</pre>
        <h2 style="font-size:28px;margin-top:12px">After you install</h2>
        <ul class="checks">
          <li>CreaBIM opens on the starter house. <em>File › New</em> starts your own project.</li>
          <li>Press <kbd>F1</kbd> over any tool for its guide page.</li>
          <li>Follow the <a href="tutorials/index.html">first-house tutorial</a>, eight short steps.</li>
          <li>Questions in <a href="{GH}/discussions">Discussions</a>; bugs in <a href="{GH}/issues">Issues</a>.</li>
        </ul>
      </div>
    </div>
  </section>
"""


def tutorials(md, texts, pages):
    steps_md = texts["start"]
    steps = re.findall(r"^\d+\.\s+\[([^\]]+)\]\(([\w-]+)\):\s*(.+)$", steps_md, re.M)
    course = "".join(f'<li><a href="../guide/{pid}.html"><span class="n">{i}</span><b>{html.escape(t)}</b><span>{html.escape(d.rstrip("."))}</span></a></li>'
                     for i, (t, pid, d) in enumerate(steps, 1))
    first = [("welcome", "New to BIM? What CreaBIM is and where to start."),
             ("starter-house", "The finished project CreaBIM opens on, and things to try in it."),
             ("interface", "The parts of the window, named."),
             ("navigate", "Orbit, pan and zoom in every view.")]
    cards = "".join(f'<a class="card" href="../guide/{p}.html"><h3>{html.escape(pages[p])}</h3><p>{d}</p><span class="more">Read →</span></a>' for p, d in first)
    return f"""
  <section class="phead gutter">
    <div class="wrap" style="display:grid;gap:18px">
      <div class="kicker">Tutorials</div>
      <h1>Learn it by <span class="acc">building.</span></h1>
      <p class="lede">Start with a first house in eight short steps, then dip into the guide for any tool. Every page here is also inside the app, one key away: <kbd>F1</kbd>.</p>
    </div>
  </section>

  <section class="gutter" style="padding-bottom:80px" id="first-house">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">01</span>Getting started</div>
        <h2>A first house, <span class="acc">step by step.</span></h2>
        <p class="lede">A small one-storey house from scratch, ending with a printed sheet and an IFC file. Each step takes a few minutes; stop at any point and come back.</p>
      </div>
      <ol class="course">{course}</ol>
      <p class="foot-note" style="margin-top:16px">Want to see a finished model first? CreaBIM opens on the starter house, and <em>File › New starter house</em> brings it back.</p>
    </div>
  </section>

  <section class="sec gutter" id="film">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">02</span>The introduction film</div>
        <h2>Three minutes, <span class="acc">one house.</span></h2>
        <p class="lede">A short tour of what CreaBIM is for: the model, the interface, profiles, documentation and open files. It lands here when it is published.</p>
      </div>
      <div class="film">
        <div class="poster" id="poster" data-soon="Film coming soon" role="img" aria-label="Introduction film: chapter card"></div>
        <ol class="chap">
          <li><span>01</span>One model, every drawing</li>
          <li><span>02</span>An interface for design</li>
          <li><span>03</span>Modelling</li>
          <li><span>04</span>Profiles</li>
          <li><span>05</span>Documentation</li>
          <li><span>06</span>Open by design</li>
          <li><span>07</span>Where it stands</li>
          <li><span>08</span>Join in</li>
        </ol>
      </div>
    </div>
  </section>

  <section class="sec gutter">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">03</span>Before you start</div>
        <h2>Find your <span class="acc">way around.</span></h2>
      </div>
      <div class="grid2">{cards}</div>
    </div>
  </section>

  <section class="sec gutter" id="guide">
    <div class="wrap">
      <div class="head">
        <div class="kicker"><span class="n">04</span>The guide</div>
        <h2>Every tool, <span class="acc">explained.</span></h2>
        <p class="lede">The complete guide from the app, by subject.</p>
      </div>
      <div class="gindex">{guide_index(pages, "../guide/")}</div>
    </div>
  </section>
"""


def guide_index(pages, prefix, current=None, side=False):
    out = []
    for sec, ids in SECTIONS.items():
        li = "".join(f'<li><a href="{prefix}{p}.html"{" aria-current=page" if p == current else ""}>{html.escape(pages[p])}</a></li>' for p in ids)
        out.append(f"<h4>{sec}</h4><ul>{li}</ul>" if side else f"<section><h3>{sec}</h3><ul>{li}</ul></section>")
    return "".join(out)


SECTIONS = {}


def shortcuts_tables(help_rs: str):
    consts = dict(re.findall(r'const (\w+): &str = "([^"]+)";', help_rs))
    block = help_rs[help_rs.index("pub const KEYS"):]
    block = block[:block.index("\n];")]
    groups = {}
    for m in re.finditer(r'key\(\s*(\w+),\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)",?\s*\)', block, re.S):
        groups.setdefault(consts.get(m.group(1), m.group(1)), []).append((m.group(2), m.group(3)))
    out = ['<p>The keys below work in the views. Menu commands show their own shortcuts beside them in the app.</p>']
    for g, keys in groups.items():
        rows = "".join(f"<tr><td style='white-space:nowrap'>{keycaps(k) if ' ' not in k or '(' in k else html.escape(k)}</td><td>{html.escape(d)}</td></tr>" for k, d in keys)
        out.append(f'<h2 id="{slug(g)}">{html.escape(g)}</h2><div class="table-wrap"><table><thead><tr><th>Key</th><th>What it does</th></tr></thead><tbody>{rows}</tbody></table></div>')
    return "".join(out)


def guide_page(pid, title, body, toc, order, pages):
    i = order.index(pid)
    sec = next(s for s, ids in SECTIONS.items() if pid in ids)
    prev = f'<a href="{order[i - 1]}.html"><span>Previous</span><b>{html.escape(pages[order[i - 1]])}</b></a>' if i > 0 else ""
    nxt = f'<a class="next" href="{order[i + 1]}.html"><span>Next</span><b>{html.escape(pages[order[i + 1]])}</b></a>' if i + 1 < len(order) else ""
    return f"""
  <div class="wrap gutter guide">
    <button class="side-toggle" type="button" aria-expanded="false">Guide contents <span aria-hidden="true">▾</span></button>
    <nav class="side" aria-label="Guide">{guide_index(pages, "", pid, side=True)}</nav>
    <article class="doc">
      <div class="crumbs"><a href="../tutorials/index.html">Tutorials</a><span>/</span><a href="../tutorials/index.html#guide">Guide</a><span>/</span><span>{html.escape(sec)}</span></div>
      {body}
      <nav class="pager" aria-label="Pages">{prev}{nxt}</nav>
    </article>
  </div>
"""


# ---------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--creabim", default=r"C:\Dev\creabim")
    ap.add_argument("--ref", default="official/main")
    a = ap.parse_args()
    repo = Path(a.creabim)
    subprocess.run(["git", "-C", str(repo), "fetch", "-q", a.ref.split("/")[0]], check=False)

    rels = releases()
    version = rels[0]["tag"].lstrip("v")
    help_rs = git_show(repo, a.ref, "crates/creabim-app/src/help.rs")
    order = []
    for sec, pid in re.findall(r'page!\("([^"]+)",\s*"([\w-]+)"\)', help_rs):
        SECTIONS.setdefault(sec, []).append(pid)
        order.append(pid)
    texts = {p: git_show(repo, a.ref, f"crates/creabim-app/help/{p}.md") for p in order}
    pages = {p: re.match(r"#\s+(.*)", t).group(1).strip() for p, t in texts.items()}
    md = Md(pages, version)

    # pictures: the guide's own, and the site's screenshots as JPEG
    (SITE / "assets/help").mkdir(parents=True, exist_ok=True)
    for name in git_ls(repo, a.ref, "crates/creabim-app/help/img"):
        (SITE / "assets/help" / name).write_bytes(git_show(repo, a.ref, f"crates/creabim-app/help/img/{name}", binary=True))
    try:
        from PIL import Image
        for png in (SITE / "assets/img").glob("*.png"):
            im = Image.open(png).convert("RGB")
            im.save(png.with_suffix(".jpg"), quality=84, optimize=True, progressive=True)
            png.unlink()
    except ImportError:
        pass

    def write(rel_path, depth, current, title, desc, body):
        p = SITE / rel_path
        p.parent.mkdir(parents=True, exist_ok=True)
        path = rel_path.replace("index.html", "")
        p.write_text(layout(depth=depth, current=current, title=title, desc=desc, body=body, path=path), encoding="utf-8")

    write("index.html", 0, "Home", None,
          "CreaBIM is a free, open-source BIM modeller: model a building in 3D, and its plans, sections, sheets, schedules, quantities and IFC follow from the same model.",
          home(rels[0]))
    write("features.html", 0, "Features", "Features",
          "What CreaBIM does: walls, slabs, roofs and structure; plans, sections, sheets and schedules from one model; IFC, glTF, FreeCAD and DXF exchange.",
          features())
    write("download.html", 0, "Download", "Download",
          f"Download CreaBIM {version} for Windows and Linux: free, open source, offline, no account.", download(rels))
    write("tutorials/index.html", 1, "Tutorials", "Tutorials",
          "Learn CreaBIM: build a first house in eight short steps, then the complete guide to every tool.", tutorials(md, texts, pages))
    for pid in order:
        title, body, toc = md.render(texts[pid])
        if pid == "shortcuts":
            body += shortcuts_tables(help_rs)
        first_p = re.search(r"<p>(.*?)</p>", body)
        desc = re.sub(r"<[^>]+>", "", first_p.group(1))[:155] if first_p else f"CreaBIM guide: {title}"
        write(f"guide/{pid}.html", 1, "Guide", title, desc, guide_page(pid, title, body, toc, order, pages))
    write("404.html", 0, None, "Page not found", "This page does not exist.",
          '<section class="final gutter"><svg class="m" aria-hidden="true"><use href="#mark"/></svg><h1 style="font-size:clamp(34px,5vw,56px)">This page isn\'t <span class="acc">built yet.</span></h1><p class="lede">The link may be old. Try the home page or the guide.</p><div class="cta"><a class="btn primary" href="/CreaBIM/">Home</a><a class="btn" href="/CreaBIM/tutorials/">Tutorials</a></div></section>')
    # 404 is served from any depth: absolute asset paths
    p404 = SITE / "404.html"
    p404.write_text(p404.read_text(encoding="utf-8").replace('href="assets/', 'href="/CreaBIM/assets/').replace('src="assets/', 'src="/CreaBIM/assets/')
                    .replace('href="index.html', 'href="/CreaBIM/index.html').replace('href="features.html', 'href="/CreaBIM/features.html')
                    .replace('href="download.html', 'href="/CreaBIM/download.html').replace('href="tutorials/', 'href="/CreaBIM/tutorials/').replace('href="guide/', 'href="/CreaBIM/guide/'), encoding="utf-8")

    urls = ["", "features.html", "download.html", "tutorials/"] + [f"guide/{p}.html" for p in order]
    (SITE / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
                                      + "".join(f"  <url><loc>{SITE_URL}{u}</loc></url>\n" for u in urls) + "</urlset>\n", encoding="utf-8")
    (SITE / "robots.txt").write_text(f"User-agent: *\nAllow: /\nSitemap: {SITE_URL}sitemap.xml\n", encoding="utf-8")
    (SITE / ".nojekyll").write_text("", encoding="utf-8")
    print(f"built {len(order) + 5} pages for CreaBIM {version}")


if __name__ == "__main__":
    main()
