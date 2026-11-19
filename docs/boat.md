# boathat — Boat fleet dashboard

This fork adds automatic Boat discovery to the existing multitop SSH grid.

## Run

Install the Boat CLI and sign in with `boat login`. Build multitop using the
upstream build script so the executable includes its Linux collectors:

```sh
./build.sh --backend zigbuild
./boathat
```

The build needs Rust, Zig and cargo-zigbuild. The upstream build script also
supports Docker. No GitHub App, separate API key, or metrics service is needed.

`BOAT_CLI` can name a different Boat executable. Otherwise the integration uses
the standard Boat installation or `boat` on PATH. `BOAT_ORG` selects the Boat
organization scope, as for the regular CLI.

## SSH hosts

To monitor another machine alongside the local tile and Boat sandboxes, add a
`[[servers]]` entry in `~/.config/multitop/boat.toml` or pass `boathat --config <path>`.
Boathat reads this file at startup. The file is optional.

```toml
[[servers]]
host = "homebox"
user = "monitor"
name = "Home box"
```

The connection uses ordinary SSH and your existing SSH keys and settings.
`port` defaults to 22. Boathat uses the connection fields and display name for
monitoring. Press `D` from the grid for Docker stats if the SSH user can read
the Docker socket. Configured hosts stay in the grid and show up as SSH rows
in the list. Lease, size and cost apply to Boat sandboxes.

## Fleet and lifecycle

- Discovery reads `boat list --all --json` every 15 seconds after the preceding
  request completes. Discovery has a 12-second timeout and never overlaps itself.
- Boats already stopped at launch are collapsed in the graph and stats grids
  and shown at the bottom of the fleet list.
- New and resumed boats inherit the current fleet stats or graph view, including
  the graph zoom. Existing graph history stays intact when the fleet changes.
- Running boats automatically appear. A visible boat that stops keeps its tile
  with a centered ASCII `DEAD` banner that scales to fit without distortion until you click it. If it resumes, its tile
  reconnects automatically.
- Click a stopped tile to dismiss it for the session. Press `c` to dismiss all
  stopped boxes from the list and grid. Press `C` to enable or disable automatic
  dismissal. It starts off each session and clears current stopped boxes when
  enabled. A later resume makes a dismissed boat visible again. Deleted boats
  disappear on the next discovery.
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

A permanent **This Mac** tile shows this machine's CPU, memory, disk, network,
processes and swap, using the native collector directly without SSH. It stays
visible when no boats are running. The fleet CPU footer counts boats only.

The full stats tile uses btop's Matcha Dark Sea palette, with a full-width CPU
graph and core readings above side-by-side memory and process panels. The
memory panel charts RAM and swap. The network and disk panels report traffic
and root disk usage. The grid adds columns as space allows. Short panes use
compact metric rows and give the remaining space to processes. Metric views never
scroll: CPU and memory graphs resize to each tile, including after a resize or
fleet change. Only process lists scroll; wheel/down moves further down the list,
and Home/End select its beginning/end. Titles show the reported **SIZE** (`default`, `large`, `xlarge`, etc.) and
**SWAP used/total**. Missing size metadata is marked `unknown`. On Linux, total is configured swap
capacity. On macOS, it is the currently allocated swap space, which can grow;
it is not a hard maximum. Unavailable readings are marked explicitly.

Press `m` to show or hide host memory and swap across stats, graphs, the fleet
list, and Fetch. `t` cycles themes. In the stats grid, use `:text #RRGGBB` to set
the text color and `:text default` to restore Matcha Dark Sea's text color.
In Stats, `1` toggles CPU, `2` toggles memory and swap, `3` toggles network,
`4` toggles processes, and `d` toggles disks. Use `D` to open Docker from
Stats. These display controls apply to the current session.

On first launch, Boathat opens in graph view, which shows CPU and memory.
Later launches restore the last List, Graphs or Stats view. Graph dots
change color with height, from the theme's healthy color through its primary
color to a pale crest. Missing history stays blank. Zoom groups samples by
peak so short CPU spikes remain visible.

The footer shows overall fleet CPU, weighted by the reporting machines'
core counts. For example, two cores at 100% and six idle cores report 25%.
Samples older than 15 seconds are excluded. The reporting count makes missing
coverage explicit; unavailable boats are not counted as zero CPU.

Use `l` to toggle the fleet list. **This Mac · Local** stays above the heavy
divider with its own CPU, memory and swap readings. Scroll the boats below it
with the mouse wheel, arrow keys, or Page Up and Page Down.

Each boat shows its state, time left on its lease, size, name, CPU and memory.
Sizes are abbreviated as S, D, L and XL. Memory uses compact binary units
such as `6.0G/16.0G`.
For running boats, **Run time** and **Compute $** show cumulative machine time
and compute cost from `boat usage`, including any previous runs of the same
sandbox. Usage refreshes once per minute while the list is open. Stopped boats
and unavailable readings show a dash. Long names wrap.

Use `G` for graphs, `s` for stats, `z` or Enter to focus a tile, `Esc` to go
back, `/` to filter by name, host or Boat ID, and `q` to quit.

## Scope

Boat mode is the interactive dashboard. It has its own SSH host file and is
separate from the standalone host dashboard, headless server and Ops poller.
Selection, themes and graph history stay in the current session.
Boat billing and snapshot metrics are not collected by the SSH system monitor.
