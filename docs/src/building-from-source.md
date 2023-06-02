# Building from Source

You can clone the repository and build Martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

```shell
git clone git@github.com:maplibre/martin.git
cd martin
cargo build --release
```

The binary will be available at `./target/release/martin`.

```shell
cd ./target/release/
./martin postgresql://postgres@localhost/db
```
