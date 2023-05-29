# Source List

A list of all available sources is available in a catalogue:

```shell
curl localhost:3000/catalog | jq
```

```yaml
[
  {
    "id": "function_zxy_query",
    "name": "public.function_zxy_query"
  },
  {
    "id": "points1",
    "name": "public.points1.geom"
  },
  ...
]
```
