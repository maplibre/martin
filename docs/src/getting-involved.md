# Getting involved

It's time to get involved once you have the [fork and all required software](development.md).
This guide covers IDE setup and debugging.
While we use Visual Studio Code as an example, Martin can be developed with any editor that supports Rust.

<details>
<summary>Editor-specific Guides (click to expand)</summary>

### Visual Studio Code

Install these essential extensions:

* [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer) - Rust language server
* [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb) - Debugger for Rust
* [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) - TOML syntax highlighting
* [GitLens](https://marketplace.visualstudio.com/items?itemName=eamodio.gitlens) - Git integration (optional)

### Vim/Neovim

Use [rustaceanvim](https://github.com/mrcjkb/rustaceanvim)

### Emacs

Use either

* [rustic](https://github.com/brotzeit/rustic) or
* [rust-mode](https://github.com/rust-lang/rust-mode)

### RustRover

[RustRover](https://jetbrains.com/rust/) supports rust out of the box

### Zed

[Zed](https://zed.dev/) supports rust out of the box

</details>

## Quick Development Setup

Before diving into IDE configuration, make sure your development environment is ready:

```bash
# Validate all required tools are installed
just validate-tools

# Start the development environment
just start  # starts test database
just help   # shows common commands
```

## Debugging with `launch.json`

Generally, you need to debug martin with specific arguments or a config file to fix issues or add features.

The most convenient way is to generate a launch.json and modify it.

## Generate

Press `F1` on your keyboard, and input "Generate Launch Configurations from Cargo.toml". Execute it and save it to your `.vscode` directory.

## Modify

Let's say you want to debug Martin with this command:

```shell
martin postgres://postgres:postgres@localhost:5411/db
```

You could find `Debug executable 'martin'` in your `launch.json`, like below:

```json
{
    "type": "lldb",
    "request": "launch",
    "name": "Debug executable 'martin'",
    "cargo": {
        "args": [
            "build",
            "--bin=martin",
            "--package=martin"
        ],
        "filter": {
            "name": "martin",
            "kind": "bin"
        }
    },
    "args": [],
    "cwd": "${workspaceFolder}"
},
```

Just copy and paste after it, and modify your pasted like this:

```javascript
{
    "type": "lldb",
    "request": "launch",
    "name": "my first debug", // name it any as you like
    "cargo": {
        "args": [
            "build",
            "--bin=martin",
            "--package=martin"
        ],
        "filter": {
            "name": "martin",
            "kind": "bin"
        }
    },
    "args": ["postgres://postgres:postgres@localhost:5411/db"], // add your arguments here
     "env": {
         "DEFAULT_SRID": 4490, // add your env here
     },
    "cwd": "${workspaceFolder}"
},
```

### Add a breakpoint

Go to any part you're interested in of martin code and add a breakpoint.

We add a breakpoint here in the [start of martin](https://github.com/maplibre/martin/blob/e628c3973f193a432d3d1282c5893e2339e806b6/martin/src/bin/martin.rs#L10).

```rust, ignore
use clap::Parser;
use tracing::{error, info};
use martin::args::{Args, OsEnv};
use martin::srv::new_server;
use martin::{read_config, Config, MartinResult};

const VERSION: &str = env!("CARGO_PKG_VERSION");

async fn start(args: Args) -> MartinResult<()> {
    info!("Starting Martin v{VERSION}");
```

### Debugging

Click `Run and Debug` on the left panel of `Visual Studio Code`. Choose `my first debug` and press `F5` on your keyboard.

Wait for the breakpoint to be hit.
