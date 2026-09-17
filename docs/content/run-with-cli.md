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

`martin --tui` turns the terminal Martin runs in into a live view of the server.
It lists the sources with how often each was asked for and how long that took, plots the last minute of tile requests on a world map, charts the request rate, and keeps the log in a pane at the bottom.
Press `q` to stop the server and `c` to reset the counters.
The dashboard needs an interactive terminal, so a service or a container keeps the plain log stream.
