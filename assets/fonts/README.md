# Fonts

Self-hosted. No CDN request is made at render time — the site works offline
and from `file://`. The `@font-face` rules and the system-stack fallbacks are
both in `style.css`, so the site renders acceptably even if these files are
missing.

All faces are the **latin** subset, pulled from Fontsource. Every one is under
the SIL Open Font License 1.1.

## crux

| File | Family | Weights | Source |
| --- | --- | --- | --- |
| `space-mono-latin-400-normal.woff2` | Space Mono | 400 | <https://fontsource.org/fonts/space-mono> |
| `space-mono-latin-700-normal.woff2` | Space Mono | 700 | <https://fontsource.org/fonts/space-mono> |
| `ibm-plex-sans-latin-wght-normal.woff2` | IBM Plex Sans | 100–700 | <https://fontsource.org/fonts/ibm-plex-sans> |
| `jetbrains-mono-latin-wght-normal.woff2` | JetBrains Mono | 100–800 | <https://fontsource.org/fonts/jetbrains-mono> |

Space Mono ships no variable build on Fontsource, so it is two static cuts
rather than one variable file.

```bash
# re-fetch, from the repo root
base=https://cdn.jsdelivr.net/npm
curl -sLo assets/fonts/space-mono-latin-400-normal.woff2 \
  "$base/@fontsource/space-mono@latest/files/space-mono-latin-400-normal.woff2"
curl -sLo assets/fonts/space-mono-latin-700-normal.woff2 \
  "$base/@fontsource/space-mono@latest/files/space-mono-latin-700-normal.woff2"
curl -sLo assets/fonts/ibm-plex-sans-latin-wght-normal.woff2 \
  "$base/@fontsource-variable/ibm-plex-sans@latest/files/ibm-plex-sans-latin-wght-normal.woff2"
curl -sLo assets/fonts/jetbrains-mono-latin-wght-normal.woff2 \
  "$base/@fontsource-variable/jetbrains-mono@latest/files/jetbrains-mono-latin-wght-normal.woff2"
```

## Why these faces

Space Mono is numerals-forward, which suits a tool whose output is read in
columns; IBM Plex Sans carries the prose without competing; JetBrains Mono is
reserved for captions, code, and data tables.
