# BootOptimPandora — agent entry point

This launcher belongs to **wbombo**, the private hub for the Wachiland Elite
startup/optimization projects: https://github.com/wachipayox/wbombo
(local workspace: `wbombo/repos/BootOptimPandora`, hub at `../../`). Read wbombo
`AGENTS.md`, `PROJECTS.md` and `proyectos/pandora.md` first. Subagent
coordination lives in wbombo `coordination/`.

Repository rules:

- `agent/integration-current` is the authority. `master` is the pinned upstream
  Moulberry/PandoraLauncher base; do not develop on it.
- Before changing launcher/profile code read
  `bootoptim-v0/BRANCH_CLEANUP_AND_CURRENT_STATUS.md` and
  `bootoptim-v0/ROADMAP.md`. Old stacked PR chains (#6–#39) are superseded;
  never merge their branches over the integrated implementation.
- The server contract is `docs/PROFILE_PROTOCOL.md` in BootOptimDistribution
  (immutable signed revisions, SHA-256 CAS, pinned inheritance, anti-rollback,
  no Start-time filesystem scan).
- Start must keep using the persistent per-instance `.minecraft`; no per-launch
  mods rebuild or whole-tree hash walk.
- Android work (launcher fork and Wachiland Android Support mod) is moving to
  its own private repositories; see wbombo `proyectos/android.md`.
- This repository is public: never commit tokens, signing keys, server
  addresses or private community data.
