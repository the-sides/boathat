# Boat fleet dashboard

This fork adds automatic Boat discovery to the existing multitop SSH grid.

## Run

Install the Boat CLI and sign in with `boat login`. Build multitop using the
upstream build script so the executable includes its Linux collectors:

```sh
./build.sh --backend zigbuild
./boat-grid
```

The build needs Rust, Zig and cargo-zigbuild. The upstream build script also
supports Docker. No GitHub App, separate API key, or metrics service is needed.

`BOAT_CLI` can name a different Boat executable. Otherwise the integration uses
the standard Boat installation or `boat` on PATH. `BOAT_ORG` selects the Boat
organization scope, as for the regular CLI.

## Fleet and lifecycle

- Discovery reads `boat list --all --json` every 15 seconds after the preceding
  request completes. Discovery has a 12-second timeout and never overlaps itself.
- Boats already stopped at launch are collapsed into the footer count.
- Running boats automatically appear. A visible boat that stops keeps its tile
  until you click it. If it resumes, its tile reconnects automatically.
- Clicking a stopped tile dismisses it for this dashboard session. A later resume
  makes it visible again. Deleted boats disappear on the next discovery.
- Sandbox IDs identify boats across name and address changes. Connections use
  `boat ssh <id>` so Boat handles SSH key authorization and current destinations.
- The dashboard never creates, resumes, stops or deletes boats automatically.
- Failed discovery keeps the previous fleet and displays a stale-inventory note.
  An incomplete paginated response is refused instead of silently losing boats.

## Metrics

The existing collector streams CPU, RAM, root disk usage, network throughput and
top processes over persistent SSH sessions. Discovery polling is separate from
metric sampling. Multitop copies its collector executable to each running boat
on first connection; it does not install a background service.

Each boat's banner shows overall CPU usage in addition to multitop's per-core
bars. The NET row also shows a total CPU bar, including when network traffic is
idle. The footer shows overall fleet CPU, weighted by the reporting machines'
core counts. For example, two cores at 100% and six idle cores report 25%.
Samples older than 15 seconds are excluded. The reporting count makes missing
coverage explicit; unavailable boats are not counted as zero CPU.

Use `G` for graphs, `z` or Enter to zoom a tile, `/` to filter by boat name or ID,
and `q` to quit. Click a stopped tile to dismiss it.

## Scope

Boat mode is the interactive dashboard; it cannot be combined with static host,
local, headless-server or Ops-poller modes. Selection, view preferences, themes
and graph history are session-only: they are neither loaded nor saved to disk.
Boat discovery owns its host list, so static-host Settings are unavailable.
Boat billing and snapshot metrics are not collected by the SSH system monitor.
