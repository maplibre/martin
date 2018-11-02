<a name="0.2.0"></a>

## 0.2.0 (2018-11-02)

#### Features

- add command-line interface ([1e128a7b](https://github.com/urbica/martin/commit/1e128a7bef484e116773d08e1e2e1a9be604aa9f))

#### Bug Fixes

- function source query params parsing ([8ac2812d](https://github.com/urbica/martin/commit/8ac2812d05ae993ea5c9013877ab4c6a1906454a))

<a name="0.1.0"></a>

## 0.1.0 (2018-11-02)

#### Bug Fixes

- rename function source query argument to query_params ([2f2b743c](https://github.com/urbica/martin/commit/2f2b743c33dcfc0f8494ec1f8a7e7c4bd0b124dc))
- pass query string from tilejson endpoint to tiles ([ef7ddace](https://github.com/urbica/martin/commit/ef7ddace17cc11433824942c2ae68ffecb00538a))
- add schema to function_sources ([a7092bc3](https://github.com/urbica/martin/commit/a7092bc3b86c35c4f7d2d14d699e1239b19d875b))
- properly encode query as json into function_sources ([cc75ab4a](https://github.com/urbica/martin/commit/cc75ab4a8e68c8291b33badb80fd8065ea4476d7))
- handle x-rewrite-url header ([63c976e8](https://github.com/urbica/martin/commit/63c976e8b9a598783150f9ef957e926d20ccf825))
- handle tables with no properties ([d6aee81b](https://github.com/urbica/martin/commit/d6aee81b1bff47a7c3f46e4c26a07a2843a9c707))
- skip tables with SRID 0 ([241dda31](https://github.com/urbica/martin/commit/241dda318453fb3bfc656793a1cef0fa6923114e))
- set default tile buffer to 64 ([612ecddb](https://github.com/urbica/martin/commit/612ecddb99f33420077dcd3f1ca0ac9666e741b6))
- revert to plain columns in tile properties request ([ea8f7aba](https://github.com/urbica/martin/commit/ea8f7abaadfd79d407a88e301e85a7ea0cd4a37d))
- use json instead of jsonb for tile request ([e6a19773](https://github.com/urbica/martin/commit/e6a19773bf523950db006538c09fbcf05124006f))
- tileset property types query ([e81cd4bb](https://github.com/urbica/martin/commit/e81cd4bb98ed77761d646a4fb82cf90ac8855963))
- remove iron-cors ([0fe335f4](https://github.com/urbica/martin/commit/0fe335f417ef27b92c538190867ab54210ef7e3a))

#### Features

- generate function_sources from database ([63114a8a](https://github.com/urbica/martin/commit/63114a8a11e0d383e8b52f428a54d0e114b3ab9d))
- add function_sources tilejson endpoint ([95d92c51](https://github.com/urbica/martin/commit/95d92c51ed14bb98f8f149d08e9a61dc02212481))
- implement function sources ([241994a5](https://github.com/urbica/martin/commit/241994a57072c3fb9c7d4344502bd9a8b6be507e))
- split sources into table_sources and function_sources ([3c3d88b1](https://github.com/urbica/martin/commit/3c3d88b1849cadc92fde969441553820259b69af))
- add config support ([c55e61d2](https://github.com/urbica/martin/commit/c55e61d27f62f2d23c7c6af4512a1a2f32dad282))
- rewrite using actix ([0080deb9](https://github.com/urbica/martin/commit/0080deb92c8b14668ee0fe6d934a1de8e3627639))
- add MVT handler ([204b132a](https://github.com/urbica/martin/commit/204b132a2699d8d1b20a7b1cabefb4e8ef749d87))
