---
icon: material/console-line
tags:
  - deployment
  - tooling
---

# Command-line Interface

You can configure Martin using command-line interface.
See `martin --help` or `cargo run -- --help` for more information:

```text
--8<-- "help/martin.txt"
```

## Terminal dashboard

Started from an interactive terminal, Martin turns it into a live view of the server.
It lists the sources with how often each was asked for and how long that took, plots the last minute of tile requests on a world map, charts the request rate, and keeps the log in a pane at the bottom.
Press `q` to stop the server and `c` to reset the counters.
The log pane writes its lines in the format `RUST_LOG_FORMAT` selects, `pretty` by default.
`martin --no-tui` prints the log stream instead, which is also what a service, a container or a pipe gets, as the dashboard needs a terminal to draw on.
