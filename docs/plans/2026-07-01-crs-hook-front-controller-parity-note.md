# crux Coordination Note: `crs` Hook Front Controller

`crux` is not the owner of the `crs` CLI or hook-profile logic.

For the 2026-07-01 front-controller migration:

- top-level hook registration moves to `crs hook ...`
- Codex keeps using `$HOME/.codex/hooks/*.crux` files as backend pipelines
- those backend pipelines continue to run through `crux run ...`

Implication for `crux`:

- no new `crs` CLI behavior should be implemented here
- no repo git-hook changes are part of this feature
- only backend contract drift in the existing `.crux` hook files would justify a
  follow-up `crux` code change
