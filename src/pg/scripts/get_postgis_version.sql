select (regexp_matches(postgis_lib_version(), '^(\d+\.\d+\.\d+)', 'g'))[1] as postgis_version;
