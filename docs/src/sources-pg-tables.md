# Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). If a [PostgreSQL connection string](pg-connections.md) is given, Martin will publish all tables as data sources if they have at least one geometry column. If geometry column SRID is 0, a default SRID must be set, or else that geo-column/table will be ignored. All non-geometry table columns will be published as vector tile feature tags (properties).
