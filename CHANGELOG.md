# Changelog

<!-- next-header -->

# ATTENTION
This file is currently not maintained. See [release](https://github.com/maplibre/martin/releases) instead.

## [Unreleased] - ReleaseDate

### ⚠ BREAKING CHANGES

* drop null serialization in tilejson endpoints ([#261](https://github.com/maplibre/martin/issues/261)) ([cee9b2b](https://github.com/maplibre/martin/commit/cee9b2bd8ca5e7303a416ada766616d864ec1f6a))

### Features

* add bounds to tilejson endpoints ([#260](https://github.com/maplibre/martin/issues/260)) (h/t [@jaspervercnocke](https://github.com/jaspervercnocke)) ([40b0a0c](https://github.com/maplibre/martin/commit/40b0a0c26aa93549fc1497faaf848049e1015070))
* add composite sources support ([#184](https://github.com/maplibre/martin/issues/184)) ([3c01125](https://github.com/maplibre/martin/commit/3c01125fc5ddb9c52aece570ae043e651c7a397c))
* add minzoom and maxzoom support ([#265](https://github.com/maplibre/martin/issues/265)) ([194a83e](https://github.com/maplibre/martin/commit/194a83e63f763323865a7f59e410e2931ce46e0a))
* add server heartbeat for monitoring the service health ([#168](https://github.com/maplibre/martin/issues/168)) (h/t [@yamaszone](https://github.com/yamaszone)) ([fce1a9e](https://github.com/maplibre/martin/commit/fce1a9e722692b24294c3766055602768112d392))
* add support for complex geometries ([#218](https://github.com/maplibre/martin/issues/218)) (h/t [@christianversloot](https://github.com/christianversloot)) ([8b5fbf9](https://github.com/maplibre/martin/commit/8b5fbf956545746d0f28e66fb73275ad46945259))
* add ssl/tls support ([#268](https://github.com/maplibre/martin/issues/268)) (h/t [@kapcsandi](https://github.com/kapcsandi)) ([7ad7f1a](https://github.com/maplibre/martin/commit/7ad7f1ab8b8fec856ca8f6f50d2ca7f897a10274))

### Bug Fixes

* fix invalid json escaping ([#224](https://github.com/maplibre/martin/issues/224)) (h/t [@gbip](https://github.com/gbip)) ([4994273](https://github.com/maplibre/martin/commit/49942734af5fcaffa4b4430e48600a0e4183d1bc))
* fix tiles attribute in tilejson when using x-rewrite-url ([#266](https://github.com/maplibre/martin/issues/266)) ([f2d56c2](https://github.com/maplibre/martin/commit/f2d56c2f7d28d858c09cab90ff13789d595ba6da))

<!-- next-url -->
[Unreleased]: https://github.com/maplibre/martin/compare/v0.5.0...HEAD

## [0.5.0](https://github.com/maplibre/martin/compare/v0.4.1...v0.5.0) (2019-10-26)


### ⚠ BREAKING CHANGES

* TileJSON scheme is XYZ by default

### Features

* upgrade actix-web to 1.0 ([#33](https://github.com/maplibre/martin/issues/33)) ([5a807e4](https://github.com/maplibre/martin/commit/5a807e40e272f6b2ccf4e7d33290521b2736b54d))


### Bug Fixes

* 🐛 change tilejson scheme to xyz by default ([aecb7ce](https://github.com/maplibre/martin/commit/aecb7ce6f45fc72034d20fcfaca186506a8dfce7)), closes [#29](https://github.com/maplibre/martin/issues/29)

### [0.4.1](https://github.com/maplibre/martin/compare/v0.4.0...v0.4.1) (2019-10-07)

### Bug Fixes

- 🐛 Fix PostGIS version check (#23) (h/t [Krizz](https://github.com/Krizz))

## [0.4.0](https://github.com/maplibre/martin/compare/v0.3.0...v0.4.0) (2019-09-30)

### ⚠ BREAKING CHANGES

- renamed CLI args: keep_alive -> keep-alive, listen_addresses ->
  listen-addresses, pool_size -> pool-size

### Bug Fixes

- 🐛 use dashes in CLI args instead of underscore ([13bec40](https://github.com/maplibre/martin/commit/13bec40)), closes [#21](https://github.com/maplibre/martin/issues/21)

## [0.3.0](https://github.com/maplibre/martin/compare/v0.2.0...v0.3.0) (2019-03-16)

#### Features

- 🎸 add watch mode for dynamic source updates #12 ([5eeef48b](https://github.com/maplibre/martin/commit/5eeef48b30ae22df83d1cff12ea1c6410e741b6b))
- 🎸 add database `connection_string` support in config ([0eb5115b](https://github.com/maplibre/martin/commit/0eb5115ba161e3d40e74fab4814d171b55de6804))

#### Bug Fixes

- 🐛 check if PostGIS is installed when starting ([e7c4dcfa](https://github.com/maplibre/martin/commit/e7c4dcfa140fa6bc774fe185cb57159eeb9062e7))

#### BREAKING CHANGES

- 💡 remove table sources filter support ([a7c17934](https://github.com/maplibre/martin/commit/a7c17934e2ea4188b2d4bd20e714441f30ea2731))

## [0.2.0](https://github.com/maplibre/martin/compare/v0.1.0...v0.2.0) (2018-11-02)

#### Features

- add command-line interface ([1e128a7b](https://github.com/maplibre/martin/commit/1e128a7bef484e116773d08e1e2e1a9be604aa9f))

#### Bug Fixes

- function source query params parsing ([8ac2812d](https://github.com/maplibre/martin/commit/8ac2812d05ae993ea5c9013877ab4c6a1906454a))

## 0.1.0 (2018-11-02)

#### Bug Fixes

- rename function source query argument to query_params ([2f2b743c](https://github.com/maplibre/martin/commit/2f2b743c33dcfc0f8494ec1f8a7e7c4bd0b124dc))
- pass query string from tilejson endpoint to tiles ([ef7ddace](https://github.com/maplibre/martin/commit/ef7ddace17cc11433824942c2ae68ffecb00538a))
- add schema to function_sources ([a7092bc3](https://github.com/maplibre/martin/commit/a7092bc3b86c35c4f7d2d14d699e1239b19d875b))
- properly encode query as json into function_sources ([cc75ab4a](https://github.com/maplibre/martin/commit/cc75ab4a8e68c8291b33badb80fd8065ea4476d7))
- handle x-rewrite-url header ([63c976e8](https://github.com/maplibre/martin/commit/63c976e8b9a598783150f9ef957e926d20ccf825))
- handle tables with no properties ([d6aee81b](https://github.com/maplibre/martin/commit/d6aee81b1bff47a7c3f46e4c26a07a2843a9c707))
- skip tables with SRID 0 ([241dda31](https://github.com/maplibre/martin/commit/241dda318453fb3bfc656793a1cef0fa6923114e))
- set default tile buffer to 64 ([612ecddb](https://github.com/maplibre/martin/commit/612ecddb99f33420077dcd3f1ca0ac9666e741b6))
- revert to plain columns in tile properties request ([ea8f7aba](https://github.com/maplibre/martin/commit/ea8f7abaadfd79d407a88e301e85a7ea0cd4a37d))
- use json instead of jsonb for tile request ([e6a19773](https://github.com/maplibre/martin/commit/e6a19773bf523950db006538c09fbcf05124006f))
- tileset property types query ([e81cd4bb](https://github.com/maplibre/martin/commit/e81cd4bb98ed77761d646a4fb82cf90ac8855963))
- remove iron-cors ([0fe335f4](https://github.com/maplibre/martin/commit/0fe335f417ef27b92c538190867ab54210ef7e3a))

#### Features

- generate function_sources from database ([63114a8a](https://github.com/maplibre/martin/commit/63114a8a11e0d383e8b52f428a54d0e114b3ab9d))
- add function_sources tilejson endpoint ([95d92c51](https://github.com/maplibre/martin/commit/95d92c51ed14bb98f8f149d08e9a61dc02212481))
- implement function sources ([241994a5](https://github.com/maplibre/martin/commit/241994a57072c3fb9c7d4344502bd9a8b6be507e))
- split sources into table_sources and function_sources ([3c3d88b1](https://github.com/maplibre/martin/commit/3c3d88b1849cadc92fde969441553820259b69af))
- add config support ([c55e61d2](https://github.com/maplibre/martin/commit/c55e61d27f62f2d23c7c6af4512a1a2f32dad282))
- rewrite using actix ([0080deb9](https://github.com/maplibre/martin/commit/0080deb92c8b14668ee0fe6d934a1de8e3627639))
- add MVT handler ([204b132a](https://github.com/maplibre/martin/commit/204b132a2699d8d1b20a7b1cabefb4e8ef749d87))
